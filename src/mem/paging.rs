//! x86-64 4 level ordinary paging

use core::{arch::asm, cell::{SyncUnsafeCell, UnsafeCell}, iter, ops::{Deref, DerefMut, Not, Range}, ptr::{addr_of, NonNull}};

use bitvec::{array::BitArray, order::Lsb0, view::BitView};
use derive_more::derive::{From, Into};
use entry::{EntryRef, EntryTarget, RawEntry};
use multiboot2::BootInformation;
use table::{RawTable, TableRef, TABLE_ALIGNMENT};

use crate::{common::hlt, drivers::vga::VGA_BUFFER, mem::{addr::AddrSpace, kernel_end_lma, kernel_end_vma, kernel_size, kernel_start_lma, kernel_start_vma, virt::KernelSpace}};

use core::fmt::Write as _;

use super::{addr::Addr, memblock::BootMemoryManager, page::{Page, PageSize, Pager}, phy, virt::{RecursivePagingSpace, VirtSpace}, LinearSpace};

mod entry;
mod table;

pub use entry::Flag;

pub trait MemoryManager {
    /// Initialize `MemoryManager`
    unsafe fn init(boot_alloc: &BootMemoryManager) -> Self;

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
    unsafe fn map<V: VirtSpace, const N: usize>(
        &self, 
        vpage: Page<V>, 
        ppage: Page<LinearSpace>, 
        flags: [Flag; N],
        allocator: &impl Pager<LinearSpace>,
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
    unsafe fn unmap<V: VirtSpace>(&self, vaddr: Addr<V>);

    /// Try translating a virtual address into a physical address. Fails iff 
    /// the virtual address is not mapped.
    fn translate<V: VirtSpace>(&self, vaddr: Addr<V>) -> Option<Addr<LinearSpace>>;
}

//---------------------------- x86-64 stuff below ---------------------------//

const DEFAULT_PAGE_TABLE_FLAGS: [Flag; 2] = [
    Flag::Present,
    Flag::ReadWrite,
];

type MemManMutex<T> = spin::Mutex<T>;
pub struct X86_64MemoryManager(MemManMutex<PageStructure>);

impl MemoryManager for X86_64MemoryManager {
    unsafe fn init(boot_alloc: &BootMemoryManager) -> Self {
        static PML4_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());
        static PDPT_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());
        static PD_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());

        let mut pml4: TableRef<'_>;
        let pdpt: TableRef<'_>;
        let mut pd: TableRef<'_>;

        unsafe {
            pml4 = TableRef::from_raw(Level::PML4, PML4_TABLE.get().as_mut_unchecked());
            pdpt = TableRef::from_raw(Level::PDPT, PDPT_TABLE.get().as_mut_unchecked());
            pd = TableRef::from_raw(Level::PD, PD_TABLE.get().as_mut_unchecked());
        }

        // Setting up kernel text mapping

        let kernel_space_start = Addr::new(KernelSpace::RANGE.start);

        let mut kernel_pml4_ent = pml4.reborrow().index_with_vaddr(kernel_space_start);
        let pdpt_vaddr = Addr::new(PDPT_TABLE.get() as usize);
        unsafe { kernel_pml4_ent.reinit(
            KernelSpace::v2p(pdpt_vaddr), 
            DEFAULT_PAGE_TABLE_FLAGS
        )}.expect("kpml4 fail");

        let mut kernel_pdpt_ent = pdpt.index_with_vaddr(kernel_space_start);
        let pd_vaddr = Addr::new(PD_TABLE.get() as usize);
        unsafe { kernel_pdpt_ent.reinit(
            KernelSpace::v2p(pd_vaddr), 
            DEFAULT_PAGE_TABLE_FLAGS
        )}.expect("kpdpt fail");

        let kernel_page_size = Level::PD.page_size().usize();
        const KERNEL_PAGE_FLAGS: [Flag; 4] = [
            Flag::Present,
            Flag::PageSize,
            Flag::Global,
            Flag::ReadWrite
        ];

        let kernel_size = kernel_size();
        let mut kernel_map_addr = kernel_space_start;
        
        while kernel_map_addr < kernel_end_vma() {
            let kernel_page_paddr = KernelSpace::v2p(kernel_map_addr);

            let mut kernel_pd_ent = pd.reborrow().index_with_vaddr(kernel_map_addr);
            unsafe { kernel_pd_ent.reinit(kernel_page_paddr, KERNEL_PAGE_FLAGS)
                .unwrap_or_else(|| panic!("kpd fail: {:#x}", kernel_page_paddr.usize())); }

            kernel_map_addr = kernel_map_addr.byte_add(kernel_page_size);
        }
        // Setting up recursive mapping

        let mut mapping_ent = pml4.index_with_vaddr(
            Addr::<RecursivePagingSpace>::new(RecursivePagingSpace::RANGE.start));

        let pml4_vaddr = Addr::new(PML4_TABLE.get() as usize);
        unsafe { mapping_ent.reinit(
            KernelSpace::v2p(pml4_vaddr), 
            DEFAULT_PAGE_TABLE_FLAGS
        )}.expect("rpml4 fail");

        let mut cr3_raw = RawEntry::default();
        unsafe { EntryRef::init(
            &mut cr3_raw, 
            Level::CR3, 
            KernelSpace::v2p(pml4_vaddr), 
            [],
        )}.expect("cr3 fail");

        let mut page_structure = PageStructure(cr3_raw);
        page_structure.store_cr3();
        let memory_manager = X86_64MemoryManager(MemManMutex::new(page_structure));

        memory_manager
    }

    unsafe fn map<V: VirtSpace, const N: usize>(
        &self, 
        vpage: Page<V>, 
        ppage: Page<LinearSpace>, 
        flags: [Flag; N],
        allocator: &impl Pager<LinearSpace>,
    ) -> Option<()> {
        debug_assert!(vpage.size() == ppage.size());


        let mut page_structure = self.0.lock();
        let mut walker = unsafe { 
            Walker::new(&mut page_structure, vpage.start()) };

        let mut cur_level = walker.cur().get_level();
        let target_level = Level::from_page_size(vpage.size());

        while cur_level != target_level {
            walker.down(allocator);
            cur_level = walker.cur().get_level();
        }

        unsafe { walker.cur().reinit(ppage.start(), flags) };
        page_structure.invalidate_tlb();

        Some(())
    }

    unsafe fn unmap<V: VirtSpace>(&self, vaddr: Addr<V>) {
        todo!()
    }

    /// Try translating a virtual address into a physical address. Fails iff 
    /// the virtual address is not mapped.
    fn translate<V: VirtSpace>(&self, vaddr: Addr<V>) -> Option<Addr<LinearSpace>> {
        todo!()
    }
}

struct Walker<'a, T: VirtSpace> {
    target_vaddr: Addr<T>, 
    cur_entry: EntryRef<'a>,
}

impl<'a, T: VirtSpace> Walker<'a, T> {
    /// Creates a new [`Walker`] to access page entries along `target_vaddr`
    /// 
    /// # Safety
    /// This walker requires recursive paging at `RecursivePagingSpace`
    unsafe fn new(
        page_structure: &'a mut PageStructure, 
        target_vaddr: Addr<T>,
    ) -> Self {
        let cur_entry = page_structure.get_cr3_ent();
        Self {
            target_vaddr,
            cur_entry 
        }
    }

    fn cur(&mut self) -> &mut EntryRef<'a> { &mut self.cur_entry }
    fn try_down(&'a mut self) -> Option<&'a mut EntryRef<'a>> {
        self.cur_entry.get_level().next_level()
            .map(|_| self.cur_entry.get_target())
            .filter(|target| matches!(target, EntryTarget::Table(..)))
            .map(|_| unsafe { self.down_unchecked() })
    }

    fn down(&mut self, alloc: &impl Pager<LinearSpace>) -> &mut EntryRef<'a> {
        if self.cur_entry.get_level().next_level().is_none() {
            return self.cur();
        }

        let target = self.cur_entry.get_target();
        match target {
            EntryTarget::None |
            EntryTarget::Page(..) => {
                let table_paddr = alloc.allocate_pages(1, PageSize::Small).unwrap().start();
                unsafe { self.cur_entry.reinit(table_paddr, DEFAULT_PAGE_TABLE_FLAGS); }
            },
            EntryTarget::Table(..) => (),
        }
        unsafe { self.down_unchecked() }
    }

    unsafe fn down_unchecked(&mut self) -> &mut EntryRef<'a> {
        let table_level = self.cur_entry.get_level().next_level()
            .expect("Walker::down_unchecked should not be called when \
                         walker is at lowest level");

        let table_vaddr = recursive_table_vaddr(table_level, self.target_vaddr);
        let raw_table: &'a mut RawTable = unsafe {table_vaddr.into_ptr::<RawTable>().as_mut_unchecked()};
        let table: TableRef<'a> = unsafe {TableRef::from_raw(table_level, raw_table)};

        self.cur_entry = table.index_with_vaddr(self.target_vaddr);
        self.cur()
    }
}

/// # Undefined Behavior
/// - `table_level` is not a valid level for page table
fn recursive_table_vaddr<S: VirtSpace>(
    table_level: Level, 
    target_addr: Addr<S>,
) -> Addr<RecursivePagingSpace> {
    assert!(!RecursivePagingSpace::RANGE.contains(&target_addr.usize()));
    const TABLE_IDX_SIZE: usize = table::TABLE_LEN.trailing_zeros() as usize;
    const OFFSET_IDX_SIZE: usize = table::TABLE_SIZE.trailing_zeros() as usize;

    let pml4_idx_range = Level::PML4.page_table_idx_range();

    let recurse_base = Addr::<RecursivePagingSpace>::new(RecursivePagingSpace::RANGE.start);
    let recurse_base = recurse_base.index_range(&pml4_idx_range);
    let recurse_base = recurse_base << pml4_idx_range.start;

    // Number of "real" page table lookup 
    let access_cnt = table_level as usize - 1;
    let recurse_cnt = 4 - access_cnt;

    let mut ret: usize = 0;
    for i in 0..recurse_cnt {
        ret |= recurse_base >> (i * TABLE_IDX_SIZE);
    }

    const OFFSET_MASK: usize = table::TABLE_ALIGNMENT - 1;
    const CANONICAL_MASK: usize = 0xFFFF_0000_0000_0000;

    let access_base = target_addr.usize() & !CANONICAL_MASK;
    let access_base = access_base >> (recurse_cnt * TABLE_IDX_SIZE);
    let access_base = access_base & !OFFSET_MASK;

    ret |= access_base;
    // RecursivePagingSpace is in upper half
    ret |= CANONICAL_MASK;

    let ret = Addr::new(ret);
    debug_assert!(ret.is_aligned_to(table::TABLE_ALIGNMENT));
    ret
}

struct PageStructure(RawEntry);
impl PageStructure {
    fn invalidate_tlb(&mut self) {
    // TODO: use invlpg instead
        unsafe {
            asm!(
                "mov {tmp}, cr3",
                "mov cr3, {tmp}",
                tmp = out(reg) _
            );
        }

    }
    fn get_cr3_ent(&mut self) -> EntryRef<'_> {
        unsafe{
            EntryRef::from_raw(&mut self.0, Level::CR3)
        }
    }

    fn load_cr3(&mut self) {
        let out: usize;
        unsafe {
            asm!("mov {}, cr3", out(reg) out);
        }
        
        // SAFETY: cr3 entry has the level CR3
        self.0 = RawEntry(out);
    }

    /// Set CR3 to the given `PageEntry`
    fn store_cr3(&mut self) {
        unsafe {
            asm!("mov cr3, {}", in(reg) self.0.0);
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    /// Control Register 3
    CR3 = 0,
    /// Page Map Level 4
    PML4 = 1,
    /// Page Directory Pointer Table
    PDPT = 2,
    /// Page Directory
    PD = 3,
    /// Page Table
    PT = 4,
}

impl Level {
    /// Get the bit range for index in a `VAddr` that indexes into a page table
    /// of this level.
    /// 
    /// # Panics
    /// panics when `self` is `CR3`, because `CR3` does not identify a page
    /// table level.
    pub const fn page_table_idx_range(self) -> Range<usize> {
        use Level::*;

        match self {
            CR3 => 
                panic!("Level::CR3 should not identify a page table level"),

            PML4 => 39..48,
            PDPT => 30..39,
            PD  => 21..30,
            PT  => 12..21,
        }
    }

    /// Get the bit range for index in a `VAddr` that indexes into a page 
    /// referenced by an `Entry` of this level.
    /// 
    /// # Panics
    /// panics when `self` is `CR3` or `PML4`, because `CR3` or `PML4` does not
    /// identify an `Entry` that references a page.
    pub const fn page_idx_range(self) -> Range<usize> {
        use Level::*;

        match self {
            CR3 | 
            PML4 => 
                panic!("Level::CR3 or Level::PML4 should not \
                        identify a page level"),
            PDPT => 0..30,
            PD => 0..21,
            PT => 0..12
        }
    }

    /// Get the page size that can be referenced by a page entry of type `self`
    /// 
    /// # Panics
    /// Panics if the `PageEntryTyp` cannot reference a page.
    pub const fn page_size(self) -> PageSize {
        use Level::*;

        match self {
            PT => PageSize::Small,
            PD => PageSize::Large,
            PDPT => PageSize::Huge,
            _ => panic!("an entry of Level cannot reference a page"),
        }   
    }

    /// Returns the type of a page entry which can reference a page of size 
    /// `page_size`
    pub const fn from_page_size(page_size: PageSize) -> Self {
        use Level::*;

        match page_size {
            PageSize::Small => PT,
            PageSize::Large => PD,
            PageSize::Huge => PDPT,
        }
    }
    /// Get next lower `Level` on the paging hierarchy, with `CR3` being
    /// the highest.
    pub const fn next_level(self) -> Option<Self> {
        use Level::*;
        match self {
            CR3 => Some(PML4),
            PML4 => Some(PDPT),
            PDPT => Some(PD),
            PD => Some(PT),
            PT => None,
        }
    }

    /// Get next higher `Level` on the paging hierarchy, with `CR3` being
    /// the highest.
    pub const fn prev_level(self) -> Option<Self> {
        use Level::*;
        match self {
            CR3 => None,
            PML4 => Some(CR3),
            PDPT => Some(PML4),
            PD => Some(PDPT),
            PT => Some(PD),
        }
    }

}