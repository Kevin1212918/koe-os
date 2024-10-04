//! x86-64 4 level ordinary paging

use core::{arch::asm, cell::SyncUnsafeCell, iter, ptr::addr_of};

use bitvec::{array::BitArray, order::Lsb0, view::BitView};

use crate::mem::{kernel_end_lma, kernel_end_vma, kernel_start_lma, kernel_start_vma, kernel_virt_to_phy, phy_to_virt, virt::IO_MAP_RANGE};

use super::{addr::{PAddr, PPage, PageSize, VAddr, VPage}, phy::FrameAllocator};

pub trait PageMapper {
    type PageSize: Ord;
    /// Returns an iterator which yields all supported page sizes in the order
    /// of smallest to largest
    fn supported_page_sizes() -> impl Iterator<Item = Self::PageSize>;
    fn is_supported(page_size: Self::PageSize) -> bool {
        Self::supported_page_sizes().any(|x|x == page_size)
    }

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
    /// - `page_size` should be supported by the `PageMap`
    unsafe fn map(
        &mut self, 
        vpage: VPage, 
        ppage: PPage, 
        allocator: &mut dyn FrameAllocator
    ) -> bool;

    /// Removes mapping at `vaddr`.
    /// 
    /// # Safety
    /// - Virtual memory page of size `page_size` pointed by `vaddr` does not 
    /// contain any live reference or owned values.
    unsafe fn unmap(&mut self, vaddr: VAddr);

    /// Try translating a virtual address into a physical address. Fails iff 
    /// the virtual address is not mapped.
    fn translate(&self, vaddr: VAddr) -> Option<PAddr>;
}

//---------------------------- x86-64 stuff below ---------------------------//

type PageTableCell = SyncUnsafeCell<PageTable>;
const fn page_table_cell() -> PageTableCell {
    SyncUnsafeCell::new(PageTable::new_uninit())
}



pub struct PageMap();
impl PageMapper for PageMap {
    type PageSize = usize;
    fn supported_page_sizes() -> impl Iterator<Item = Self::PageSize> {
        [4096].into_iter()
    }
    /// Initialize `PageMap`
    /// 
    /// # Safety
    /// Should be called with interrupt disabled
    unsafe fn init() -> Self {
        use PageEntryTyp::*;
        use PageFlag::*;

        static PML4_TABLE: PageTableCell = page_table_cell();
        static KERNEL_PDP_TABLE: PageTableCell = page_table_cell();
        static KERNEL_PD_TABLE: PageTableCell = page_table_cell();

        let pml4; 
        let k_pdp; 
        let k_pd; 

        unsafe {
            pml4 = PML4_TABLE.get().as_mut_unchecked();
            k_pdp = KERNEL_PDP_TABLE.get().as_mut_unchecked();
            k_pd = KERNEL_PD_TABLE.get().as_mut_unchecked();
        }

        let kernel_addr = kernel_start_vma();

        // Map the kernel pdp to pml4
        pml4.get_entry_mut(CR3, kernel_addr)
            .init(PML4, unsafe {kernel_virt_to_phy(VAddr::from_ref(k_pdp))})
            .expect("physical address of k_pdp should be aligned")
            .set_flags(PML4, [ReadWrite, Global]).then_some(())
            .expect("ReadWrite and Global flags should be valid for PML4 entry");
        // Map the kernel pd to pml4
        k_pdp.get_entry_mut(PML4, kernel_addr)
            .init(PDPT {is_page: false}, unsafe {kernel_virt_to_phy(VAddr::from_ref(k_pd))})
            .expect("physical address of k_pd should be aligned")
            .set_flags(PDPT { is_page: false }, [ReadWrite, Global]).then_some(())
            .expect("ReadWrite and Global flags should be valid for PDPT entry");
        
        // Map the kernel text space to entries of pd
        let mut page_addr = kernel_start_lma();
        while page_addr < kernel_end_lma() {
            k_pd.get_entry_mut(PDPT { is_page: false }, kernel_addr)
                .init(PD { is_page: true }, page_addr)
                .expect("physical address of kernel_start should be aligned")
                .set_flags(PD { is_page: true }, [ReadWrite, Global]).then_some(())
                .expect("ReadWrite and Global flags should be valid for PML4 entry");
                let Some(new_addr) = page_addr.checked_byte_add(0x200000) else {
                    break;
                };
                page_addr = new_addr;
        }
        let pml4_addr = unsafe { kernel_virt_to_phy(VAddr::from_ref(pml4)) };
        let cr3 = PageEntry::new(CR3, pml4_addr)
            .expect("pml4 should be a valid CR3 entry");
        unsafe {set_cr3(cr3);}

        PageMap()
    }
    unsafe fn map(
        &mut self, 
        vpage: VPage, 
        ppage: PPage, 
        allocator: &mut dyn FrameAllocator
    ) -> bool {
        use PageEntryTyp::*;
        const ENTRY_LEVELS: [PageEntryTyp; 5] = [
            CR3, 
            PML4,
            PDPT { is_page: true },
            PD { is_page: true },
            PT
        ];
        assert_eq!(vpage.size, ppage.size);
        let mut cr3 = get_cr3();

        let mut cur_entry_level_idx = 0;
        let mut cur_entry_level = ENTRY_LEVELS[cur_entry_level_idx];
        let mut cur_entry = &mut cr3;
        let target_level = PageEntryTyp::from_page_size(vpage.size);

        while cur_entry_level != target_level {
            if cur_entry.
            let next_addr = phy_to_virt(cur_entry.get_ref_addr(cur_entry_level));


        }

        true
    }
    
    unsafe fn unmap(&mut self, vaddr: VAddr) {
        todo!()
    }
    
    fn translate(&self, vaddr: VAddr) -> Option<PAddr> {
        todo!()
    }

}

fn get_cr3() -> PageEntry {
    let out: usize;
    unsafe {
        asm!("mov {}, cr3", out(reg) out);
    }
    PageEntry(out)
}
unsafe fn set_cr3(ent: PageEntry) {
    let b = ent.0;
    unsafe {
        asm!("mov cr3, {}", in(reg) b);
    }
}


/// A paging table
#[repr(C, align(4096))]
pub struct PageTable(pub [PageEntry; 4096]);
impl PageTable {
    const fn new_uninit() -> Self {
        PageTable([PageEntry::new_uninit(); 4096])
    }
    /// For a `PageTable` of the given `typ`, get the `PageEntry` indexed by 
    /// `addr`
    fn get_entry_mut(&mut self, table_typ: PageEntryTyp, addr: VAddr) -> &mut PageEntry {
        let idx = table_typ.page_table_idx_of(addr);
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
    /// Use `page_size` bit to populate `typ` with uninitalized `is_page`. Does
    /// not do anything if `typ` does not have `is_page` flag. Note that 
    /// is_page` from input is ignored.
    pub fn extract_is_page(&self, typ: &mut PageEntryTyp) {
        use PageEntryTyp::*;
        match typ {
            CR3 | PML4 | PT => (),
            PD { is_page } |
            PDPT { is_page } => {
                let pg_bit = *(unsafe { self.0.view_bits::<Lsb0>().get_unchecked(7) });
                *is_page = pg_bit
            }
        }
    }
    pub fn get_ref_addr(&self, typ: PageEntryTyp) -> PAddr {
        use PageEntryTyp::*;

        // Clear out 64:48 and 11:0
        let mut addr = 0x0000_FFFF_FFFF_F000 & self.0;

        // If a huge page, clear out bit 12
        match typ {
            PDPT{is_page: true} |
            PD{is_page: true} => { addr = addr & !(0x1000); }
            _ => ()
        }
        unsafe {PAddr::from_usize(addr)}
    }
    pub fn set_ref_addr(&mut self, typ: PageEntryTyp, addr: PAddr) -> bool {
        use PageEntryTyp::*;

        let alignment = match typ {
            CR3 | PML4 | PT |
            PDPT { is_page: false } | 
            PD { is_page: false } => 0x1000,
            PD { is_page: true } => 0x200000,
            PDPT { is_page: true } => 0x40000000, 
        };

        if addr.into_usize() % alignment != 0 {
            return false;
        }

        // Keeps 64:alignment and alignment:0
        let mask = 0x0000_FFFF_FFFF_FFFF;
        let mask = mask - (alignment - 1);
        let mask = !mask;

        self.0 = (self.0 & mask) + addr.into_usize();

        true
    }
    
    pub fn get_flag(&self, typ: PageEntryTyp, flag: PageFlag) -> Option<bool> {
        let idx = typ.flag_idx_of(flag)?;
        let bitfield = self.0.view_bits::<Lsb0>();

        Some(unsafe { *(bitfield.get_unchecked(idx)) })
    }
    
    pub fn set_flag(
        &mut self, 
        typ: PageEntryTyp, 
        flag: PageFlag, 
        value: bool
    ) -> bool {
        let Some(idx) = typ.flag_idx_of(flag) else {
            return false 
        };
        let bitfield = self.0.view_bits_mut::<Lsb0>();

        unsafe { bitfield.set_unchecked(idx, value) };
        true
    }

    pub fn set_flags(
        &mut self,
        typ: PageEntryTyp,
        flags: impl IntoIterator<Item = PageFlag>
    ) -> bool {
        flags.into_iter().map(|flag| {
            self.set_flag(typ, flag, true)
        }).all(|b|b)
    }

    const fn new_uninit() -> Self {
        PageEntry(0)
    }
    /// Initialize an uninitialized `PageEntry`
    pub fn init(
        &mut self,
        typ: PageEntryTyp, 
        addr: PAddr, 
    ) -> Option<&mut Self> {
        use PageEntryTyp::*;

        match typ {
            CR3 => (),

            PT | PML4 |
            PD {is_page: false} |
            PDPT { is_page: false } => {
                self.set_flag(typ, PageFlag::Present, true);
            },

            PD {is_page: true} |
            PDPT {is_page: true} => {
                self.set_flag(typ, PageFlag::Present, true);
                self.set_flag(typ, PageFlag::PageSize, true);
            }
        }
        self.set_ref_addr(typ, addr);
        Some(self)
    }
    pub fn uninit(&mut self) { self.0 = 0 }
    /// Create a new `PageEntry`. 
    pub fn new(
        typ: PageEntryTyp, 
        addr: PAddr, 
    ) -> Option<Self> {
        let mut ent = PageEntry(0);
        ent.init(typ, addr)?;
        Some(ent)
    }

}

/// A flag in a page entry. Currently supports `Present`, `ReadWrite`, 
/// `UserSuper`, `PageSize`, `Global`.
#[derive(Debug, Clone, Copy)]
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
    PD{is_page: bool},
    /// Page Directory Pointer Table
    PDPT{is_page: bool},
    /// Page Map Level 4
    PML4,
}

impl PageEntryTyp {
    /// Get the index/offset into entry of the given type from `addr`.
    const fn page_table_idx_of(&self, addr: VAddr) -> usize {
        let addr: usize = addr.into_usize();
        let (bit_start, bit_end): (usize, usize) = match self {
            PageEntryTyp::CR3 => (39, 48) ,
            PageEntryTyp::PML4 => (30, 39),

            PageEntryTyp::PDPT { is_page: false } => (30, 21),
            PageEntryTyp::PDPT { is_page: true } => (30, 0),

            PageEntryTyp::PD { is_page: false } => (21, 12),
            PageEntryTyp::PD { is_page: true } => (21, 0),

            PageEntryTyp::PT => (12, 0)
        };
        let addr_len = bit_end - bit_start;
        let mask = (2usize << addr_len) - 1;

        (addr >> bit_start) & mask
    }
    const fn flag_idx_of(&self, flag: PageFlag) -> Option<usize> {
        use PageFlag::*;
        use PageEntryTyp::*;
        
        // SAFETY: 7 is less than size of usize
        match (self, flag) {
            // (CR3, WriteThru) => Some(3),
            // (CR3, CacheDisable) => Some(4),
            (CR3, _) => None,

            (_, Present) => Some(0),
            (_, ReadWrite) => Some(1),
            (_, UserSuper) => Some(2),
            // (_, WriteThru) => Some(3),
            // (_, CacheDisable) => Some(4),
            // (_, Accessed) => Some(5),

            (PD{..}, PageSize) |
            (PDPT{..}, PageSize) => Some(7),

            // (PT, Dirty) |
            // (PD{is_page: true}, Dirty) |
            // (PDPT{is_page: true}, Dirty) => Some(6),
            
            // (PT, PageAttrTbl) => Some(7),
            // (PD{is_page: true}, PageAttrTbl) |
            // (PDPT{is_page: true}, PageAttrTbl) => Some(12),

            (PT, Global) |
            (PD{is_page: true}, Global) |
            (PDPT{is_page: true}, Global) => Some(8),

            _ => None
        }
    }
    const fn page_size(&self) -> Option<PageSize> {
        match self {
            PageEntryTyp::PT => Some(PageSize::Small),
            PageEntryTyp::PD { is_page: true } => Some(PageSize::Large),
            PageEntryTyp::PDPT { is_page: true } => Some(PageSize::Huge),
            _ => None
        }   
    }
    const fn from_page_size(page_size: PageSize) -> Self {
        match page_size {
            PageSize::Small => PageEntryTyp::PT,
            PageSize::Large => PageEntryTyp::PD { is_page: true },
            PageSize::Huge => PageEntryTyp::PDPT { is_page: true },
        }
    }

}