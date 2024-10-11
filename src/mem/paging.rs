//! x86-64 4 level ordinary paging

use core::{arch::asm, cell::SyncUnsafeCell, iter, ops::Not, ptr::addr_of};

use bitvec::{array::BitArray, order::Lsb0, view::BitView};
use multiboot2::BootInformation;

use crate::{drivers::vga::VGA_BUFFER, mem::{kernel_end_lma, kernel_end_vma, kernel_start_lma, kernel_start_vma, kernel_virt_to_phy, phy_to_virt}};

use core::fmt::Write as _;

use super::{addr::{KiB, PAddr, PPage, PageSize, VAddr, VPage}, phy::{self, Allocator}};

pub fn init<'boot>() {
    unsafe { PageStructure::init() };
}
pub trait MemoryManager {
    /// Initialize `MemoryManager`
    /// 
    /// # Safety
    /// Should be called with interrupt disabled
    unsafe fn init() -> Self;

    /// Maps a virtual page of size `page_size` to `paddr`. Overwrite any 
    /// previous virtual page mapping at `vaddr`. 
    /// 
    /// # Safety
    /// - Virtual memory page of size `page_size` pointed by `vaddr` does not 
    /// contain any live reference or owned values.
    /// - Physical memory page of size `page_size` pointed by `paddr` does not 
    /// contain any live reference or owned values.
    /// 
    /// # Panics
    /// - `page_size` should be supported by the `MemoryManager`
    unsafe fn map(
        &mut self, 
        vpage: VPage, 
        ppage: PPage, 
        allocator: &mut dyn phy::Allocator
    ) -> Option<()>;

    /// Removes mapping at `vaddr`.
    /// 
    /// # Safety
    /// - `vaddr` is the base of a mapped `VPage` through this `MemoryManager`
    /// - The previously mapped virtual memory page pointed by `vaddr` does not 
    /// contain any live reference or owned values.
    /// 
    /// # Panics
    /// May panic if `vaddr` is not mapped.
    unsafe fn unmap(&mut self, vaddr: VAddr);

    /// Try translating a virtual address into a physical address. Fails iff 
    /// the virtual address is not mapped.
    fn translate(&self, vaddr: VAddr) -> Option<PAddr>;
}

//---------------------------- x86-64 stuff below ---------------------------//


type PageTableCell = SyncUnsafeCell<PageTable>;
const fn page_table_cell() -> PageTableCell {
    SyncUnsafeCell::new(PageTable::new())
}

pub struct PageStructure(spin::Mutex<()>);
impl MemoryManager for PageStructure {
    unsafe fn init() -> Self {
        use PageEntryTyp::*;
        use PageFlag::*;

        static PML4_TABLE: PageTableCell = page_table_cell();
        static KERNEL_PDP_TABLE: PageTableCell = page_table_cell();
        static KERNEL_PD_TABLE: PageTableCell = page_table_cell();

        let pml4: &mut PageTable; 
        let k_pdp: &mut PageTable; 
        let k_pd: &mut PageTable; 
        
        let pml4_addr: PAddr; 
        let k_pdp_addr: PAddr; 
        let k_pd_addr: PAddr; 

        unsafe {
            pml4 = PML4_TABLE.get().as_mut_unchecked();
            k_pdp = KERNEL_PDP_TABLE.get().as_mut_unchecked();
            k_pd = KERNEL_PD_TABLE.get().as_mut_unchecked();

            pml4_addr = kernel_virt_to_phy(VAddr::from_ref(pml4));
            k_pdp_addr = kernel_virt_to_phy(VAddr::from_ref(k_pdp));
            k_pd_addr = kernel_virt_to_phy(VAddr::from_ref(k_pd));
        }

        let kernel_addr = kernel_start_vma();

        // Map the kernel pdp to pml4
        unsafe { 
            pml4.get_entry_mut(PML4, kernel_addr)
                .init(PML4, k_pdp_addr, [Present, ReadWrite]);
        };
        // Map the kernel pd to pml4
        unsafe {
            k_pdp.get_entry_mut(PDPT, kernel_addr)
                .init(PDPT, k_pd_addr, [Present, ReadWrite]);
        }
        // assert!(kernel_end_lma() > kernel_start_lma());
        // Map the kernel text space to entries of pd
        const PD_PAGE_SIZE: usize = PD.page_size().into_usize();
        let mut page_addr = PAddr::from(0);
            // PAddr::from(kernel_start_lma().into_usize() - kernel_start_lma().into_usize() % PD_PAGE_SIZE);
        while page_addr < kernel_end_lma() {

            unsafe {
                k_pd.get_entry_mut(PD, kernel_addr)
                    .init(PD, page_addr, [Present, ReadWrite, PageSize]);
            }

            let Some(new_addr) = page_addr.checked_byte_add(PD_PAGE_SIZE) else {
                break;
            };
            page_addr = new_addr;
        }
        let mut cr3 = PageEntry::new_cleared();
        unsafe {cr3.init(CR3, pml4_addr, [])};
        unsafe {set_cr3(cr3);}

        // Setting up identity paging

        PageStructure(spin::Mutex::new(()))
    }

    unsafe fn map(
        &mut self, 
        vpage: VPage, 
        ppage: PPage, 
        allocator: &mut dyn phy::Allocator
    ) -> Option<()> {
        // Only support one flag for now
        use PageEntryTyp::*;

        const PAGE_TABLE_FLAGS: [PageFlag; 3] = 
            [PageFlag::Present, PageFlag::ReadWrite, PageFlag::Global];

        assert_eq!(vpage.size(), ppage.size());
        let mut cr3 = get_cr3();

        let mut cur_entry_typ = CR3;
        let mut cur_entry = &mut cr3;
        let target_typ = PageEntryTyp::from_page_size(vpage.size());

        while cur_entry_typ != target_typ {
            // SAFETY: cur_entry_typ and PAGE_TABLE_FLAGS specify a page table
            unsafe { init_table_if_not_table(
                cur_entry, 
                cur_entry_typ, 
                allocator, 
                PAGE_TABLE_FLAGS
            )};
            let next_addr = phy_to_virt(cur_entry.get_ref_addr(cur_entry_typ));

            // SAFETY: cur_entry should reference a page table from 
            // `init_table_if_not_table`. 
            let next_table = unsafe { 
                next_addr.into_ptr::<PageTable>().as_mut_unchecked() 
            };

            // An entry references the table one level beneath the entry.
            cur_entry_typ = cur_entry_typ.next_level()
                .expect("cur_entry_typ should not be at the lowest level\
                        because target_typ has not been found");
            cur_entry = next_table.get_entry_mut(cur_entry_typ, vpage.start());
        }

        // SAFETY: cur_entry_typ == target_typ which specify a page. 
        // PAGE_TABLE_FLAGS also specify page.
        unsafe {
            match cur_entry_typ {
                CR3 | PML4 => panic!("CR3/PML4 should not reference a page"),
                PT => init_page_if_not_page(
                    cur_entry, 
                    cur_entry_typ, 
                    allocator, 
                    [
                        PageFlag::Present, 
                        PageFlag::ReadWrite, 
                        PageFlag::Global
                    ]
                ),
                PD | PDPT => init_page_if_not_page(
                    cur_entry,
                    cur_entry_typ, 
                    allocator, 
                    [
                        PageFlag::Present, 
                        PageFlag::ReadWrite, 
                        PageFlag::PageSize, 
                        PageFlag::Global
                    ]
                ),
            };
        }

        invalidate_tlb();
        return Some(());

        /// Reinitialize the entry to references a newly allocated page table
        /// if the entry does not reference a table
        /// 
        /// # Safety 
        /// Should ensure `typ` and `flag` specify a page entry which reference
        /// a page table
        unsafe fn init_table_if_not_table<const N: usize>(
            ent: &mut PageEntry, 
            typ: PageEntryTyp,
            allocator: &mut dyn phy::Allocator,
            flags: [PageFlag; N]
        ) -> Option<()> {
            if ent.is_present(typ) && !ent.is_page(typ) {
                return Some(());
            }

            let mut new_ent = PageEntry::new_cleared();

            const {assert!(PageTable::TABLE_SIZE < PageSize::Small.into_usize())};
            let page = allocator.allocate_page(PageSize::Small)?;
            let page_table_ptr = phy_to_virt(page.start()).into_ptr::<PageTable>();
            let page_table = PageTable::new();

            // SAFETY: page_table_ptr is mapped in virtual space, and 
            // have enough room for a page table as guarenteed by 
            // the allocator.
            unsafe { page_table_ptr.write(page_table) };

            // SAFETY: Caller should ensure `typ` and `flag` specify a page 
            // entry which reference a page table. `page.base` points to 
            // the top of a page which holds a page table.
            unsafe { new_ent.init(typ, page.start(), flags) };
            *ent = new_ent;

            Some(())
        }

        /// Reinitialize the entry to references a newly allocated page if the 
        /// entry does not reference a page
        /// 
        /// # Safety 
        /// Should ensure `typ` and `flag` specify a page entry which reference
        /// a page
        unsafe fn init_page_if_not_page<const N: usize>(
            ent: &mut PageEntry, 
            typ: PageEntryTyp,
            allocator: &mut dyn phy::Allocator,
            flags: [PageFlag; N]
        ) -> Option<()> {
            if ent.is_present(typ) && ent.is_page(typ) {
                return Some(());
            }

            let mut new_ent = PageEntry::new_cleared();
            let page = allocator.allocate_page(typ.page_size())?;

            // SAFETY: Caller should ensure typ and flags specify a page
            // entry which reference a page. `page.base` points to a page 
            // of size typ.page_size by construction.
            unsafe { new_ent.init(typ, page.start(), flags) };
            *ent = new_ent;

            Some(())
        }
    }
    
    unsafe fn unmap(&mut self, vaddr: VAddr) {
        use PageEntryTyp::*;
        let mut cr3 = get_cr3();

        let mut cur_entry_typ = CR3;
        let mut cur_entry = &mut cr3;

        loop {
            if !cur_entry.is_present(cur_entry_typ) {
                panic!("Mapper::unmap should be called on mapped addr");
            }
            if cur_entry.is_page(cur_entry_typ) {
                // Found the page
                break;
            }
            assert!(cur_entry_typ.next_level().is_some());

            let next_addr = phy_to_virt(cur_entry.get_ref_addr(cur_entry_typ));

            // SAFETY: cur_entry should reference a page table since if it is
            // not present or a page, it wouldve panicked or break out
            let next_table = unsafe { 
                next_addr.into_ptr::<PageTable>().as_mut_unchecked() 
            };

            // An entry references the table one level beneath the entry.
            cur_entry_typ = cur_entry_typ.next_level()
                .expect("cur_entry_typ should not be at the lowest level\
                        because target_typ has not been found");
            cur_entry = next_table.get_entry_mut(cur_entry_typ, vaddr);
        }

        cur_entry.clear();
        invalidate_tlb();
    }
    
    fn translate(&self, vaddr: VAddr) -> Option<PAddr> {
        todo!()
    }

}

fn invalidate_tlb() {
    // TODO: use invlpg instead
    unsafe {
        asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
             tmp = out(reg) _
        );
    }

}
fn get_cr3() -> PageEntry {
    let out: usize;
    unsafe {
        asm!("mov {}, cr3", out(reg) out);
    }
    PageEntry(out)
}

/// Set CR3 to the given `PageEntry`
/// 
/// # Safety
/// Should ensure `ent` points to a valid paging structure and uphold all
/// kernel invariances.
unsafe fn set_cr3(ent: PageEntry) {
    let b = ent.0;
    unsafe {
        asm!("mov cr3, {}", in(reg) b);
    }
}

// Workaround to ensure alginement of PAGE_TABLE_SIZE for PageTable
const _: () = assert!(PageTable::TABLE_ALIGNMENT == 4 * KiB);
/// A paging table
#[repr(C, align(4096))]
pub struct PageTable(pub [PageEntry; Self::TABLE_LEN]);
impl PageTable {
    const TABLE_SIZE: usize = 4 * KiB;
    const TABLE_LEN: usize = Self::TABLE_SIZE / size_of::<PageEntry>();
    const TABLE_ALIGNMENT: usize = Self::TABLE_SIZE;

    const fn new() -> Self {
        PageTable([PageEntry::new_cleared(); Self::TABLE_LEN])
    }
    
    /// For a `PageTable` of the given `typ`, get the `PageEntry` indexed by 
    /// `addr`
    fn get_entry_mut(&mut self, table_typ: PageEntryTyp, addr: VAddr) -> &mut PageEntry {
        let idx = table_typ.page_table_idx_from(addr);
        debug_assert!(idx < self.0.len());
        unsafe {self.0.get_unchecked_mut(idx)}
    }
}

/// A paging table entry.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct PageEntry(usize);
impl PageEntry {
    pub fn is_present(&self, typ: PageEntryTyp) -> bool {
        // CR3 should be the only entry without present flag, and it is 
        // always present
        self.get_flag(typ, PageFlag::Present).unwrap_or(true)
    }
    /// Check if current page entry's address refers to a page. Returns false
    /// if present bit is false.
    pub fn is_page(&self, typ: PageEntryTyp) -> bool {
        use PageEntryTyp::*;
        match typ {
            CR3 | PML4 => false,
            PT => true,
            PD | PDPT => { 
                self.get_flag(typ, PageFlag::PageSize)
                    .unwrap_or(false)
            }
        }
    }
    
    
    pub fn get_ref_addr(&self, typ: PageEntryTyp) -> PAddr {
        use PageEntryTyp::*;

        let mut addr = self.0;

        // Clear out 64:48 and 11:0; if a large/huge page, clear out bit 12 
        // as well
        let mask = match typ {
            PDPT | PD => {
                let is_page = self.get_flag(typ, PageFlag::PageSize)
                    .expect("PDPT/PD should have PageSize flag");
                if is_page {
                    0x0000_FFFF_FFFF_E000
                } else {
                    0x0000_FFFF_FFFF_F000
                }
            },
            _ => 0x0000_FFFF_FFFF_F000,
        };

        addr = mask & addr;
        unsafe {PAddr::from_usize(addr)}
    }
    
    /// # Safety
    /// Should ensure an initialized page entry references a page/page table as
    /// specified by its flags.
    pub fn set_ref_addr(&mut self, typ: PageEntryTyp, addr: PAddr) -> bool {
        use PageEntryTyp::*;

        let alignment = match typ {
            CR3 | PML4 | PT => 0x1000,
            PD => {
                if !self.is_page(typ) {
                    PageTable::TABLE_ALIGNMENT
                } else {
                    PD.page_size().into()
                }
            },
            PDPT => {
                if !self.is_page(typ) {
                    PageTable::TABLE_ALIGNMENT
                } else {
                    PDPT.page_size().into()
                }
            }
        };

        if addr.into_usize() % alignment != 0 {
            return false;
        }

        // Build a mask which keeps 64:alignment and alignment:0
        let mask = 0x0000_FFFF_FFFF_FFFF;
        let mask = mask - (alignment - 1);
        let mask = !mask;

        self.0 = (self.0 & mask) + addr.into_usize();

        true
    }

    fn get_flag_idx(&self, typ: PageEntryTyp, flag: PageFlag) -> Option<usize> {
        use PageFlag::*;
        use PageEntryTyp::*;

        match (typ, flag) {
            (CR3, _) => return None,
            (PML4, Present) |
            (PDPT, Present) |
            (PD, Present) |
            (PT, Present) => return Some(0),
            _ => ()
        }
        let is_present = self.get_flag(typ, Present)
            .expect("PML4/PDPT/PD/PT should have present flag");
        if !is_present {
            return None;
        }

        match (typ, flag) {
            (CR3, _) => None,

            (PML4, ReadWrite) => Some(1),
            (PML4, UserSuper) => Some(2),
            (PML4, _) => None,

            (PDPT, ReadWrite) => Some(1),
            (PDPT, UserSuper) => Some(2),
            (PDPT, PageSize) => Some(7),
            (PDPT, Global) => {
                let is_page = self.get_flag(typ, PageSize)
                    .expect("PDPT should have PageSize flag");
                is_page.then_some(8)
            },
            (PDPT, _) => None,

            (PD, ReadWrite) => Some(1),
            (PD, UserSuper) => Some(2),
            (PD, PageSize) => Some(7),
            (PD, Global) => {
                let is_page = self.get_flag(typ, PageSize)
                    .expect("PD should have PageSize flag");
                is_page.then_some(8)
            },
            (PD, _) => None,
            
            (PT, ReadWrite) => Some(1),
            (PT, UserSuper) => Some(2),
            (PT, Global) => Some(8),
            (PT, _) => None,
        }
    }
    pub fn get_flag(&self, typ: PageEntryTyp, flag: PageFlag) -> Option<bool> {
        let idx = self.get_flag_idx(typ, flag)?;
        let bitfield = self.0.view_bits::<Lsb0>();

        Some(unsafe { *(bitfield.get_unchecked(idx)) })
    }
    
    /// # Safety
    /// Should only set `Present` and `PageSize` flags if `self` is 
    /// uninitialized
    pub unsafe fn set_flag(
        &mut self, 
        typ: PageEntryTyp, 
        flag: PageFlag, 
        value: bool
    ) -> bool {
        let Some(idx) = self.get_flag_idx(typ, flag) else {
            return false 
        };
        let bitfield = self.0.view_bits_mut::<Lsb0>();

        unsafe { bitfield.set_unchecked(idx, value) };
        true
    }

    /// # Safety
    /// Should only set `Present` and `PageSize` flags if `self` is 
    /// uninitialized
    pub unsafe fn set_flags<const N: usize>(
        &mut self,
        typ: PageEntryTyp,
        mut flags: [PageFlag; N],
    ) -> bool {
        // Sorting is needed to ensure Present flag is set before accessing other
        // flags, and that PageSize flag is set before accessing relevent flags
        flags.as_mut_slice().sort_unstable();
        flags.into_iter().map(|flag| {
            // SAFETY: Caller should ensure the safety condition.
            unsafe {self.set_flag(typ, flag, true)}
        }).all(|b|b)
    }
    /// Create a cleared `PageEntry`
    pub const fn new_cleared() -> Self { Self(0) }
    /// Initialize an uninitialized `PageEntry`.
    /// 
    /// # Safety
    /// `addr` should point to a page table/page as specified by a `PageEntry`
    /// of `typ` and `flags`
    pub unsafe fn init<const N: usize>(
        &mut self, 
        typ: PageEntryTyp, 
        addr: PAddr, 
        flags: [PageFlag; N]
    ) -> bool {
        // Need to set flags before address to ensure entry type is parsed
        // correctly
        // SAFETY: self is uninitialized so all flags can be set. Caller should
        // ensure addr points to a page table/page as specified by typ and 
        // flags
        self.clear();
        unsafe {self.set_flags(typ, flags) && self.set_ref_addr(typ, addr)}
    }
    pub fn clear(&mut self) { self.0 = 0; }
}

/// A flag in a page entry. Currently supports `Present`, `ReadWrite`, 
/// `UserSuper`, `PageSize`, `Global`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageFlag {
    // Universal set_flags  
    Present,
    ReadWrite,
    UserSuper,
    // WriteThru,
    // CacheDisable,
    // Accessed,
    
    // Table/Page
    PageSize,

    // Page flags
    // Dirty,
    // PageAttrTbl,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEntryTyp {
    /// Control Register 3
    CR3,
    /// Page Table
    PT,
    /// Page Directory
    PD,
    /// Page Directory Pointer Table
    PDPT,
    /// Page Map Level 4
    PML4,
}

impl PageEntryTyp {
    /// Get the index into the page table of `typ` from the virtual address 
    /// `addr`
    /// 
    /// # Panics
    /// panics when `self` is `CR3`, because `CR3` does not identify a page
    /// table level.
    const fn page_table_idx_from(self, addr: VAddr) -> usize {
        let addr: usize = addr.into_usize();
        let (bit_start, bit_end): (usize, usize) = match self {
            PageEntryTyp::CR3 => 
                panic!("PageEntryTyp::CR3 should not identify a page table type"),

            PageEntryTyp::PML4 => (39, 48),
            PageEntryTyp::PDPT => (30, 39),
            PageEntryTyp::PD  => (21, 30),
            PageEntryTyp::PT  => (12, 21),
        };
        let addr_len = bit_end - bit_start;
        let mask = (1usize << addr_len) - 1;

        (addr >> bit_start) & mask
    }

    /// Get the offset into the page referenced by an page entry of `typ` from 
    /// the virtual address `addr`
    /// 
    /// # Panics
    /// panics when `self` is `CR3` or `PML4`, because `CR3` or `PML4` does not
    /// identify a page frame size.
    const fn page_frame_idx_from(self, addr: VAddr) -> usize {
        let addr: usize = addr.into_usize();
        let addr_len: usize = match self {
            PageEntryTyp::CR3 | 
            PageEntryTyp::PML4 => 
                panic!("PageEntryTyp::CR3 or PageEntryTyp::PML4 should not \
                        identify a page frame type"),
            PageEntryTyp::PDPT => 30,
            PageEntryTyp::PD => 21,
            PageEntryTyp::PT => 12
        };
        let mask = (2usize << addr_len) - 1;

        addr & mask
    }

    /// Get the page size that can be referenced by a page entry of type `self`
    /// 
    /// # Panics
    /// Panics if the `PageEntryTyp` cannot reference a page.
    const fn page_size(self) -> PageSize {
        match self {
            PageEntryTyp::PT => PageSize::Small,
            PageEntryTyp::PD => PageSize::Large,
            PageEntryTyp::PDPT => PageSize::Huge,
            _ => panic!("an entry of PageEntryTyp cannot reference a page"),
        }   
    }

    /// Returns the type of a page entry which can reference a page of size 
    /// `page_size`
    const fn from_page_size(page_size: PageSize) -> Self {
        match page_size {
            PageSize::Small => PageEntryTyp::PT,
            PageSize::Large => PageEntryTyp::PD,
            PageSize::Huge => PageEntryTyp::PDPT,
        }
    }
    /// Get next lower `PageEntryTyp` on the paging hierarchy, with `CR3` being
    /// the highest.
    const fn next_level(self) -> Option<Self> {
        use PageEntryTyp::*;
        match self {
            CR3 => Some(PML4),
            PML4 => Some(PDPT),
            PDPT => Some(PD),
            PD => Some(PT),
            PT => None,
        }
    }

}