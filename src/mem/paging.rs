//! x86-64 4 level ordinary paging

use alloc::alloc::Global;
use alloc::boxed::Box;
use core::alloc::{Allocator, Layout};
use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;
use core::ops::{DerefMut, Range};
use core::ptr::NonNull;
use core::sync::atomic::AtomicBool;

use arraydeque::RangeArgument;
use entry::{EntryRef, EntryTarget, RawEntry};
use table::{RawTable, TableRef};

use super::addr::{Addr, PageAddr, PageManager, PageSize};
use super::phy::BootMemoryManager;
use super::virt::{PhysicalRemapSpace, RecursivePagingSpace, VirtSpace};
use super::{PageAllocator, UMASpace};
use crate::mem::addr::AddrSpace;
use crate::mem::virt::{DataStackSpace, KernelSpace};
use crate::mem::{kernel_end_vma, kernel_size};

mod entry;
mod table;

pub use entry::Flag;

pub trait MemoryManager {
    type Map: MemoryMap;

    /// Initialize `MemoryManager`.
    ///
    /// This also creates an initial memory map.
    fn init(bmm: &BootMemoryManager) -> Self;

    /// Swap out the current memory map with `new`.
    fn swap(&self, new: Self::Map) -> Self::Map;

    /// Borrow the current memory map.
    fn map(&self) -> impl DerefMut<Target = Self::Map>;

    /// Flush the changes to current memory map.
    fn flush(&self);
}

pub trait MemoryMap {
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
        &mut self,
        vpage: PageAddr<V>,
        ppage: PageAddr<UMASpace>,
        flags: [Flag; N],
        alloc: &mut impl PageManager<UMASpace>,
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
    unsafe fn unmap<V: VirtSpace>(&mut self, vaddr: Addr<V>);

    /// Try translating a virtual address into a physical address. Fails iff
    /// the virtual address is not mapped.
    fn translate<V: VirtSpace>(&mut self, vaddr: Addr<V>) -> Option<Addr<UMASpace>>;
}

//---------------------------- x86-64 stuff below ---------------------------//

pub static MMU: spin::Once<X86_64MemoryManager> = spin::Once::new();

const DEFAULT_PAGE_TABLE_FLAGS: [Flag; 3] = [Flag::Present, Flag::Global, Flag::ReadWrite];

pub struct X86_64MemoryManager(spin::Mutex<X86_64MemoryMap>);

impl MemoryManager for X86_64MemoryManager {
    type Map = X86_64MemoryMap;

    fn init(bmm: &BootMemoryManager) -> Self {
        static PML4_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());
        static KERNEL_PDPT_TABLE: SyncUnsafeCell<RawTable> =
            SyncUnsafeCell::new(RawTable::default());
        static KERNEL_PD_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());

        static LINEAR_PAGE_TABLE: SyncUnsafeCell<RawTable> =
            SyncUnsafeCell::new(RawTable::default());

        unsafe fn insert_unsafe_cell_ent<const N: usize, S: VirtSpace>(
            table: &SyncUnsafeCell<RawTable>,
            table_level: Level,
            ent_addr: Addr<UMASpace>,
            flags: [Flag; N],
            target: Addr<S>,
        ) {
            let table_ref = unsafe {
                TableRef::from_raw(
                    table_level,
                    table.get().as_mut_unchecked(),
                )
            };
            let mut table_ent = table_ref.index_with_vaddr(target);
            unsafe { table_ent.reinit(ent_addr, flags) }.expect("fail");
        }

        fn init_physical_remap(bmm: &BootMemoryManager, kernel_map_end: Addr<KernelSpace>) {
            // Setting up a 2MB page table at the end of kernel to hold PhysicalRemapSpace
            // tables.
            let page_table_pd_start = kernel_map_end;
            let ent_addr = KernelSpace::v2p(Addr::new(
                LINEAR_PAGE_TABLE.get() as usize
            ));
            unsafe {
                insert_unsafe_cell_ent(
                    &KERNEL_PD_TABLE,
                    Level::PD,
                    ent_addr,
                    DEFAULT_PAGE_TABLE_FLAGS,
                    page_table_pd_start,
                )
            };

            // cur_pdpt_vaddr is incremented immediately in the first iteration of the loop
            let mut cur_remap_table_vaddr = page_table_pd_start - PageSize::Small.usize();
            let remap_start = Addr::<PhysicalRemapSpace>::new(PhysicalRemapSpace::RANGE.start);
            let remap_max_size = usize::min(
                bmm.managed_range().size,
                PhysicalRemapSpace::RANGE.end - PhysicalRemapSpace::RANGE.start,
            );
            let mut remap_vaddr_offset: usize = 0;

            // PhysicalRemapSpace are huge pages.
            while remap_vaddr_offset + PageSize::Huge.usize() <= remap_max_size {
                let cur_remap_vaddr = remap_start + remap_vaddr_offset;

                let pml4_ref = unsafe { PML4_TABLE.get().as_mut_unchecked() };
                let pml4_ref = unsafe { TableRef::from_raw(Level::PML4, pml4_ref) };

                let mut pml4_ent = pml4_ref.index_with_vaddr(cur_remap_vaddr);
                if !pml4_ent.is_present() {
                    cur_remap_table_vaddr = cur_remap_table_vaddr + PageSize::Small.usize();
                    let cur_remap_table_paddr = bmm
                        .allocate_pages(1, PageSize::Small)
                        .expect("Direct remap alloc fail")
                        .base
                        .addr();
                    unsafe {
                        insert_unsafe_cell_ent(
                            &LINEAR_PAGE_TABLE,
                            Level::PT,
                            cur_remap_table_paddr,
                            KERNEL_PAGE_FLAGS,
                            cur_remap_table_vaddr,
                        )
                    };
                    unsafe {
                        pml4_ent.reinit(
                            cur_remap_table_paddr,
                            DEFAULT_PAGE_TABLE_FLAGS,
                        )
                    };
                }

                let cur_remap_table_ref = unsafe {
                    cur_remap_table_vaddr
                        .into_ptr::<RawTable>()
                        .as_mut_unchecked()
                };
                let cur_remap_table_ref =
                    unsafe { TableRef::from_raw(Level::PDPT, cur_remap_table_ref) };
                let mut cur_remap_page_ref = cur_remap_table_ref.index_with_vaddr(cur_remap_vaddr);
                unsafe {
                    cur_remap_page_ref.reinit(
                        PhysicalRemapSpace::v2p(cur_remap_vaddr),
                        KERNEL_PAGE_FLAGS,
                    )
                };

                remap_vaddr_offset = remap_vaddr_offset + PageSize::Huge.usize();
            }
        }

        // Setting up kernel text mapping
        let kernel_space_start = Addr::new(KernelSpace::RANGE.start);

        let ent_addr = KernelSpace::v2p(Addr::new(
            KERNEL_PDPT_TABLE.get() as usize
        ));
        unsafe {
            insert_unsafe_cell_ent(
                &PML4_TABLE,
                Level::PML4,
                ent_addr,
                DEFAULT_PAGE_TABLE_FLAGS,
                kernel_space_start,
            )
        };

        let ent_addr = KernelSpace::v2p(Addr::new(KERNEL_PD_TABLE.get() as usize));
        unsafe {
            insert_unsafe_cell_ent(
                &KERNEL_PDPT_TABLE,
                Level::PDPT,
                ent_addr,
                DEFAULT_PAGE_TABLE_FLAGS,
                kernel_space_start,
            )
        };

        let kernel_page_size = Level::PD.page_size().usize();
        const KERNEL_PAGE_FLAGS: [Flag; 4] =
            [Flag::Present, Flag::PageSize, Flag::Global, Flag::ReadWrite];

        let mut kernel_map_addr = kernel_space_start;
        while kernel_map_addr < kernel_end_vma() {
            let kernel_page_paddr = KernelSpace::v2p(kernel_map_addr);
            unsafe {
                insert_unsafe_cell_ent(
                    &KERNEL_PD_TABLE,
                    Level::PD,
                    kernel_page_paddr,
                    KERNEL_PAGE_FLAGS,
                    kernel_map_addr,
                )
            };

            kernel_map_addr = kernel_map_addr + kernel_page_size;
        }

        init_physical_remap(bmm, kernel_map_addr);

        let pml4_vaddr = Addr::new(PML4_TABLE.get() as usize);
        let mut cr3_raw = RawEntry::default();
        unsafe {
            EntryRef::init(
                &mut cr3_raw,
                Level::CR3,
                KernelSpace::v2p(pml4_vaddr),
                [],
            )
        }
        .expect("cr3 fail");

        let map = X86_64MemoryMap { cr3: cr3_raw };
        let memory_manager = X86_64MemoryManager(spin::Mutex::new(map));

        memory_manager
    }

    fn swap(&self, new: Self::Map) -> Self::Map {
        let mut map = self.0.lock();
        core::mem::replace(map.deref_mut(), new)
    }

    fn map(&self) -> impl DerefMut<Target = Self::Map> { self.0.lock() }

    fn flush(&self) { flush_tlb(); }
}

fn set_cr3(entry: RawEntry) { unsafe { asm!("mov cr3, {}", in(reg) entry.0) }; }
fn cr3() -> RawEntry {
    let out: usize;
    unsafe { asm!("mov {}, cr3", out(reg) out) };
    RawEntry(out)
}
fn flush_tlb() {
    // TODO: use invlpg instead
    unsafe {
        asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
            tmp = out(reg) _
        );
    }
}

/// A memory mapping that is represented by a cr3 entry.
///
/// The cr3 entry points to a PML4 table, which holds both kernel and userspace
/// mapping. Kernel mapping is shared across all [`X86_64MemoryMap`]s, and is
/// not dropped when [`X86_64MemoryMap`] is dropped.
///
/// FIXME: When map operations preempt each other, multiple mutable references
/// to kernel page table may exist at the same time.
pub struct X86_64MemoryMap {
    cr3: RawEntry,
}
impl X86_64MemoryMap {
    fn new(mmu: &mut X86_64MemoryManager) -> Self {
        let cr3 = RawEntry::default();
        let table_ptr = PageAllocator
            .allocate(Layout::new())
            .expect("Allocation failed!");
        let table_vaddr: Addr<PhysicalRemapSpace> = Addr::new(table_ptr.as_ptr().cast() as usize);
        let table_paddr = PhysicalRemapSpace::v2p(table_vaddr);

        let pml4_table_ref = unsafe {
            TableRef::from_raw(
                Level::PML4,
                table_vaddr.into_ptr::<RawTable>().as_mut_unchecked(),
            )
        };

        // Copy over kernel pages
        // TODO: Fix hardcoded idxs for kernel pages.
        let cur_table: TableRef = mmu.map().deref_mut().into();
        pml4_table_ref.entries()[256..].copy_from_slice(&cur_table.entries()[256..]);

        let cr3_ref = unsafe {
            EntryRef::init(
                &mut cr3,
                Level::CR3,
                table_paddr,
                DEFAULT_PAGE_TABLE_FLAGS,
            )
        }
        .expect("Flags should be valid");
        Self { cr3 }
    }
}
impl MemoryMap for X86_64MemoryMap {
    unsafe fn map<V: VirtSpace, const N: usize>(
        &mut self,
        vpage: PageAddr<V>,
        ppage: PageAddr<UMASpace>,
        flags: [Flag; N],
        allocator: &mut impl PageManager<UMASpace>,
    ) -> Option<()> {
        debug_assert!(vpage.page_size() == ppage.page_size());

        let mut walker = unsafe { LinearWalker::new(self.into(), vpage.start()) };

        let mut cur_level = walker.cur().level();
        let target_level = Level::from_page_size(vpage.page_size());

        while cur_level != target_level {
            walker.down(allocator);
            cur_level = walker.cur().level();
        }

        unsafe { walker.cur().reinit(ppage.start(), flags) };

        Some(())
    }


    unsafe fn unmap<V: VirtSpace>(&mut self, vaddr: Addr<V>) { todo!() }

    fn translate<V: VirtSpace>(&mut self, vaddr: Addr<V>) -> Option<Addr<UMASpace>> {
        let mut walker = unsafe { LinearWalker::new(self.into(), vaddr) };

        while walker.try_down().is_some() {}

        match walker.cur().target() {
            EntryTarget::None => None,
            EntryTarget::Page(_, addr) => Some(addr),
            EntryTarget::Table(..) => unreachable!(),
        }
    }
}
impl Drop for X86_64MemoryMap {
    fn drop(&mut self) {
        // Dont call this on kernel page!
        fn drop_entry_target(ent: EntryRef<'_>) {
            let EntryTarget::Table(level, addr) = ent.target() else {
                return;
            };
            let table_ptr = PhysicalRemapSpace::p2v(addr).into_ptr::<RawTable>();
            let raw_table = unsafe { table_ptr.as_mut_unchecked() };
            let table = unsafe { TableRef::from_raw(level, raw_table) };
            for raw_entry in table.entries() {
                let entry = unsafe { EntryRef::from_raw(raw_entry, level) };
                drop_entry_target(entry);
            }
            PageAllocator.deallocate(
                unsafe { NonNull::new_unchecked(table_ptr) }.cast(),
                Layout::new::<RawTable>(),
            );
        }

        let pml4_table: TableRef<'_> = self.into();
        for raw_entry in &mut pml4_table.entries()[256..] {
            let entry = unsafe { EntryRef::from_raw(raw_entry, Level::PML4) };
            drop_entry_target(entry);
        }
        PageAllocator.deallocate(
            unsafe { NonNull::new_unchecked(pml4_table.into() as *mut RawTable) }.cast(),
            Layout::new::<RawTable>(),
        );
    }
}
impl<'a> Into<EntryRef<'a>> for &'a mut X86_64MemoryMap {
    fn into(self) -> EntryRef<'a> { unsafe { EntryRef::from_raw(&mut self.cr3, Level::CR3) } }
}
impl<'a> Into<TableRef<'a>> for &'a mut X86_64MemoryMap {
    fn into(self) -> TableRef<'a> {
        let ent: EntryRef<'a> = self.into();
        let EntryTarget::Table(level, addr) = ent.target() else { unreachable!() };
        let table_vaddr = PhysicalRemapSpace::p2v(addr);
        let raw_table = unsafe { table_vaddr.into_ptr::<RawTable>().as_mut_unchecked() };
        unsafe { TableRef::from_raw(level, raw_table) }
    }
}

struct LinearWalker<'a, T: VirtSpace> {
    target_vaddr: Addr<T>,
    cur_entry: EntryRef<'a>,
}
impl<'a, T: VirtSpace> LinearWalker<'a, T> {
    /// Creates a new [`LinearWalker`] to access page entries along
    /// `target_vaddr`
    ///
    /// # Safety
    /// This walker requires table physical addresses mapped at
    /// `PhysicalRemapSpace`.
    unsafe fn new(cr3: EntryRef<'a>, target_vaddr: Addr<T>) -> Self {
        Self {
            target_vaddr,
            cur_entry: cr3,
        }
    }

    fn cur(&mut self) -> &mut EntryRef<'a> { &mut self.cur_entry }

    fn try_down(&mut self) -> Option<&mut EntryRef<'a>> {
        self.cur_entry
            .level()
            .next_level()
            .and_then(|_| match self.cur_entry.target() {
                EntryTarget::Table(level, addr) =>
                    Some(unsafe { self.down_with_table(addr, level) }),
                _ => None,
            })
    }

    /// Moves walker down.
    ///
    /// If walker is at the last level, do nothing. If next level of walker is
    /// unmapped, create a new table, and then move down.
    fn down(&mut self, alloc: &mut impl PageManager<UMASpace>) -> &mut EntryRef<'a> {
        if self.cur_entry.level().next_level().is_none() {
            return self.cur();
        }

        let target = self.cur_entry.target();
        match target {
            EntryTarget::None | EntryTarget::Page(..) => {
                let table_paddr = alloc.allocate_pages(1, PageSize::Small).unwrap().base;
                let table_level = self.cur_entry.level().next_level().unwrap();
                unsafe {
                    self.cur_entry.reinit(
                        table_paddr.into(),
                        DEFAULT_PAGE_TABLE_FLAGS,
                    );
                }
                unsafe { self.down_with_table(table_paddr.addr(), table_level) }
            },
            EntryTarget::Table(level, addr) => unsafe { self.down_with_table(addr, level) },
        }
    }

    // # Safety
    //
    // `table_paddr` and `table_level` are from the [`EntryTarget`] of the current
    // entry.
    unsafe fn down_with_table(
        &mut self,
        table_paddr: Addr<UMASpace>,
        table_level: Level,
    ) -> &mut EntryRef<'a> {
        let table_vaddr = PhysicalRemapSpace::p2v(table_paddr);

        let raw_table = unsafe { table_vaddr.into_ptr::<RawTable>().as_mut_unchecked() };
        let table: TableRef<'a> = unsafe { TableRef::from_raw(table_level, raw_table) };

        self.cur_entry = table.index_with_vaddr(self.target_vaddr);
        self.cur()
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
            CR3 => panic!("Level::CR3 should not identify a page table level"),

            PML4 => 39..48,
            PDPT => 30..39,
            PD => 21..30,
            PT => 12..21,
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
            CR3 | PML4 => panic!("Level::CR3 or Level::PML4 should not identify a page level"),
            PDPT => 0..30,
            PD => 0..21,
            PT => 0..12,
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

/// A physical memory allocator to hold page table.
///
/// This allocator interfaces with raw address instead of page frame because
/// this is initialized before the global page frames is initialized.
///
/// # Safety
/// Implementor should ensure [`PageTableAllocator::allocate`] returns memory
/// which fits the page table, and both functions follow the usual requirements
/// specified in [`alloc::alloc::Allocator`].
pub unsafe trait PageTableAllocator {
    fn allocate(&self) -> Addr<UMASpace>;
    fn deallocate(&self, addr: Addr<UMASpace>);
}

// ------------------------- Unused -----------------------------
//
// struct RecursiveWalker<'a, T: VirtSpace> {
//     target_vaddr: Addr<T>,
//     cur_entry: EntryRef<'a>,
// }
//
// impl<'a, T: VirtSpace> RecursiveWalker<'a, T> {
//     /// Creates a new [`RecursiveWalker`] to access page entries along
// `target_vaddr`     ///
//     /// # Safety
//     /// This walker requires recursive paging at `RecursivePagingSpace`, and
// the     /// paging structure pointed by `cr3` is currently loaded.
//     unsafe fn new(cr3: EntryRef<'a>, target_vaddr: Addr<T>) -> Self {
//         Self {
//             target_vaddr,
//             cur_entry: cr3,
//         }
//     }
//
//     fn cur(&mut self) -> &mut EntryRef<'a> { &mut self.cur_entry }
//
//     fn try_down(&mut self) -> Option<&mut EntryRef<'a>> {
//         self.cur_entry
//             .level()
//             .next_level()
//             .map(|_| self.cur_entry.target())
//             .filter(|target| matches!(target, EntryTarget::Table(..)))
//             .map(|_| unsafe { self.down_unchecked() })
//     }
//
//     fn down(&mut self, alloc: &mut impl PageManager<UMASpace>) -> &mut
// EntryRef<'a> {         if self.cur_entry.level().next_level().is_none() {
//             return self.cur();
//         }
//
//         let target = self.cur_entry.target();
//         match target {
//             EntryTarget::None | EntryTarget::Page(..) => {
//                 let table_paddr = alloc.allocate_pages(1,
// PageSize::Small).unwrap().base;                 unsafe {
//                     self.cur_entry.reinit(
//                         table_paddr.into(),
//                         DEFAULT_PAGE_TABLE_FLAGS,
//                     );
//                 }
//             },
//             EntryTarget::Table(..) => (),
//         }
//         unsafe { self.down_unchecked() }
//     }
//
//     unsafe fn down_unchecked(&mut self) -> &mut EntryRef<'a> {
//         let table_level =
//             self.cur_entry.level().next_level().expect(
//                 "RecursiveWalker::down_unchecked should not be called when
// walker is at lowest level",             );
//
//         let table_vaddr = recursive_table_vaddr(table_level,
// self.target_vaddr);         let raw_table: &'a mut RawTable =
//             unsafe { table_vaddr.into_ptr::<RawTable>().as_mut_unchecked() };
//         let table: TableRef<'a> = unsafe { TableRef::from_raw(table_level,
// raw_table) };
//
//         self.cur_entry = table.index_with_vaddr(self.target_vaddr);
//         self.cur()
//     }
// }
//
// /// # Undefined Behavior
// /// - `table_level` is not a valid level for page table
// fn recursive_table_vaddr<S: VirtSpace>(
//     table_level: Level,
//     target_addr: Addr<S>,
// ) -> Addr<RecursivePagingSpace> {
//     assert!(!RecursivePagingSpace::RANGE.contains(&target_addr.usize()));
//     const TABLE_IDX_SIZE: usize = table::TABLE_LEN.trailing_zeros() as usize;
//     const OFFSET_IDX_SIZE: usize = table::TABLE_SIZE.trailing_zeros() as
// usize;
//
//     let pml4_idx_range = Level::PML4.page_table_idx_range();
//
//     let recurse_base =
// Addr::<RecursivePagingSpace>::new(RecursivePagingSpace::RANGE.start);     let
// recurse_base = recurse_base.index_range(&pml4_idx_range);
//     let recurse_base = recurse_base << pml4_idx_range.start;
//
//     // Number of "real" page table lookup
//     let access_cnt = table_level as usize - 1;
//     let recurse_cnt = 4 - access_cnt;
//
//     let mut ret: usize = 0;
//     for i in 0..recurse_cnt {
//         ret |= recurse_base >> (i * TABLE_IDX_SIZE);
//     }
//
//     const OFFSET_MASK: usize = table::TABLE_ALIGNMENT - 1;
//     const CANONICAL_MASK: usize = 0xFFFF_0000_0000_0000;
//
//     let access_base = target_addr.usize() & !CANONICAL_MASK;
//     let access_base = access_base >> (recurse_cnt * TABLE_IDX_SIZE);
//     let access_base = access_base & !OFFSET_MASK;
//
//     ret |= access_base;
//     // RecursivePagingSpace is in upper half
//     ret |= CANONICAL_MASK;
//
//     let ret = Addr::new(ret);
//     debug_assert!(ret.is_aligned_to(table::TABLE_ALIGNMENT));
//     ret
// }
//
