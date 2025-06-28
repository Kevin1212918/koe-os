//! x86-64 4 level ordinary paging

use alloc::sync::Arc;
use core::alloc::{Allocator, Layout};
use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;
use core::ops::{Deref, Range};
use core::ptr::{self};

use arraydeque::RangeArgument;
use derive_more::derive::From;
use entry::{EntryRef, EntryTarget, RawEntry};
use table::{RawTable, TableRef};

use super::addr::{self, Addr, Page, PageSize};
use super::phy::BootMemoryManager;
use super::virt::{PhysicalRemapSpace, VirtSpace};
use super::{PageAllocator, UMASpace};
use crate::mem::addr::{AddrSpace, Allocator as _};
use crate::mem::kernel_end_vma;
use crate::mem::virt::KernelImageSpace;

mod entry;
mod table;

pub use entry::Flags;

pub fn init(bmm: &BootMemoryManager) -> impl MemoryManager { x86_64_init(bmm) }

/// Smart shared reference to a memory map.
#[derive(Clone, From)]
pub enum MapRef<M: MemoryMap> {
    Static(&'static M),
    Arc(Arc<M>),
}
impl<M: MemoryMap> Deref for MapRef<M> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        match self {
            MapRef::Static(r) => r,
            MapRef::Arc(r) => r,
        }
    }
}

/// Per-CPU structure managing the CPU's page mapping.
///
/// A `MemoryManager` always holds reference to one `MemoryMap`, which contains
/// the active memory mapping. On initialization, `MemoyManager` starts with a
/// map already loaded.
pub trait MemoryManager {
    type Map: MemoryMap;

    /// Swap out the current memory map with `new`.
    ///
    /// # Safety
    /// While this function is safe, the caller should uphold the safety
    /// guarentees from [`MemoryMap::map`].
    fn swap(&mut self, new: MapRef<Self::Map>);

    /// Borrow the current memory map.
    fn map(&mut self) -> &Self::Map;

    /// Flush the changes to current memory map.
    ///
    /// # Note
    /// This should be called when the kernel mapping or the currently mapped
    /// user mapping is changed.
    fn flush(&mut self);
}

/// A swappable set of page mappings.
///
/// A `MemoryMap` reference is held by [`MemoryManager`] to determine the
/// active user mapping.
///
/// Kernel page mapping are shared across `MemoryMap`s. Any modification to
/// kernel mapping in a map will reflect to all other maps. User mappings are
/// unique to a `MemoryMap`, which can be swapped using [`MemoryManager::swap`]
///
/// Note that since a `MemoryMap` will be modified from multiple cores, it
/// implements [`Sync`] and [`Send`]
pub trait MemoryMap: 'static + Sync + Send {
    /// Create a new `MemoryMap`.
    ///
    /// # Panics
    /// - If [`paging::init`] has not returned, this will panic.
    fn new() -> Self;

    /// Maps a virtual page of size `page_size` to `paddr`. Overwrite any
    /// previous virtual page mapping at `vaddr`.
    ///
    /// # Safety
    /// Paging operations are fundamentally unsafe. All of Rust's safety
    /// guarentees should be justified when calling this function, among
    /// those:
    /// - Mutable references cannot alias,
    /// - References points to valid values.
    ///
    /// Note however the user processes may not need such strong guarentees,
    /// unless they are explicitly accessed in kernel code.
    ///
    /// # Panics
    /// - `page_size` should be supported by the `MemoryManager`. We cant do
    ///   large pages yet.
    unsafe fn map<V: VirtSpace, const N: usize>(
        &self,
        vpage: Page<V>,
        ppage: Page<UMASpace>,
        flags: Flags,
        alloc: &mut impl addr::Allocator<UMASpace>,
    ) -> Option<()>;

    /// Removes mapping at `vaddr`.
    ///
    /// # Safety
    /// See [`MemoryMap::map`] for safety requirements.
    ///
    /// # Undefined Behavior
    /// The page at `vaddr` should be mapped.
    unsafe fn unmap<V: VirtSpace>(&self, vaddr: Addr<V>);

    /// Try translating a virtual address into a physical address. Fails iff
    /// the virtual address is not mapped.
    fn translate<V: VirtSpace>(&self, vaddr: Addr<V>) -> Option<Addr<UMASpace>>;
}

//---------------------------- x86-64 stuff below ---------------------------//

pub static MMU: spin::Once<X86_64MemoryManager> = spin::Once::new();
// TODO: Use RAII to guard kernel mappings.
pub static KERNEL_MAP_LOCK: spin::Mutex<()> = spin::Mutex::new(());

const DEFAULT_PAGE_TABLE_FLAGS: Flags = Flags::PRESENT.union(Flags::WRITEABLE);

const KERNEL_TABLES_CNT: usize = 256;

static DEFAULT_KERNEL_MAP: spin::Once<X86_64MemoryMap> = spin::Once::new();

/// Initalize kernel paging and memory managers.
///
/// Need to static initialize some starting pages before physical pages can be
/// mapped dynamically.
///
/// All of PhysicalRemapSpace, KernelImageSpace, and kernel pdpts are statically
/// allocated and should not be returned to allocators.
fn x86_64_init(bmm: &BootMemoryManager) -> X86_64MemoryManager {
    static PML4_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());
    static PDPT_TABLES: SyncUnsafeCell<[RawTable; KERNEL_TABLES_CNT]> =
        SyncUnsafeCell::new([const { RawTable::default() }; KERNEL_TABLES_CNT]);

    fn init_kernel_pdpt(pdpt_ref: TableRef<'_>) {
        static KERNEL_PD_TABLE: SyncUnsafeCell<RawTable> = SyncUnsafeCell::new(RawTable::default());

        let kernel_space_start = Addr::new(KernelImageSpace::RANGE.start);
        let mut pdpt_ent_ref = pdpt_ref.index_with_vaddr(kernel_space_start);
        pdpt_ent_ref.reinit(
            KernelImageSpace::v2p(Addr::new(KERNEL_PD_TABLE.get() as usize)),
            DEFAULT_PAGE_TABLE_FLAGS,
        );

        const KERNEL_PAGE_SIZE: PageSize = Level::PD.page_size();
        const KERNEL_PAGE_FLAGS: Flags = Flags::PRESENT
            .union(Flags::BIG_PAGE)
            .union(Flags::GLOBAL)
            .union(Flags::WRITEABLE);

        let mut pd_ref = unsafe {
            TableRef::from_raw(
                Level::PD,
                KERNEL_PD_TABLE.get().as_mut_unchecked(),
            )
        };
        let mut kernel_page_vaddr = kernel_space_start;
        while kernel_page_vaddr < kernel_end_vma() {
            let kernel_page_paddr = KernelImageSpace::v2p(kernel_page_vaddr);
            let mut pd_ent_ref = pd_ref.reborrow().index_with_vaddr(kernel_page_vaddr);
            pd_ent_ref.reinit(kernel_page_paddr, KERNEL_PAGE_FLAGS);

            kernel_page_vaddr = kernel_page_vaddr + KERNEL_PAGE_SIZE.usize();
        }
    }

    fn init_physical_remap_pdpt(pdpt_ref: TableRef<'_>, remap_idx: usize) {
        const REMAP_PAGE_FLAGS: Flags = Flags::PRESENT
            .union(Flags::BIG_PAGE)
            .union(Flags::GLOBAL)
            .union(Flags::WRITEABLE);
        const REMAP_PAGE_SIZE: PageSize = Level::PDPT.page_size();
        let remap_start = remap_idx * (REMAP_PAGE_SIZE.usize() * table::TABLE_LEN);

        for (idx, mut pdpt_ent_ref) in pdpt_ref.entry_refs().into_iter().enumerate() {
            let remap_paddr = Addr::new(remap_start + (idx * REMAP_PAGE_SIZE.usize()));
            pdpt_ent_ref.reinit(remap_paddr, REMAP_PAGE_FLAGS);
        }
    }

    let pdpt_table_iter = unsafe {
        PDPT_TABLES
            .get()
            .as_mut_unchecked()
            .iter_mut()
            .map(|x| TableRef::from_raw(Level::PDPT, x))
    };
    for (idx, mut table) in pdpt_table_iter.enumerate() {
        // offset the idx by KERNEL_TABLES_CNT since the preallocated pdpts are for
        // kernel pages.
        let idx = idx + KERNEL_TABLES_CNT;

        let kernel_page_idx = Addr::<KernelImageSpace>::new(KernelImageSpace::RANGE.start)
            .index_range(&Level::PML4.page_table_idx_range());
        let remap_page_start = Addr::<PhysicalRemapSpace>::new(PhysicalRemapSpace::RANGE.start)
            .index_range(&Level::PML4.page_table_idx_range());
        let remap_page_end = usize::min(
            PhysicalRemapSpace::RANGE.end - 1,
            PhysicalRemapSpace::RANGE.start + bmm.managed_range().size - 1,
        );
        let remap_page_end = Addr::<PhysicalRemapSpace>::new(remap_page_end)
            .index_range(&Level::PML4.page_table_idx_range());

        if idx == kernel_page_idx {
            init_kernel_pdpt(table.reborrow());
        } else if remap_page_start <= idx && idx <= remap_page_end {
            init_physical_remap_pdpt(table.reborrow(), idx - remap_page_start);
        }

        let pdpt_table_vaddr = Addr::new(ptr::from_mut(table.raw()) as usize);
        let pdpt_table_paddr = KernelImageSpace::v2p(pdpt_table_vaddr);

        let pml4_ref = unsafe {
            TableRef::from_raw(
                Level::PML4,
                PML4_TABLE.get().as_mut_unchecked(),
            )
        };
        let mut pml4_ent_ref = pml4_ref.index(idx);
        pml4_ent_ref.reinit(
            pdpt_table_paddr,
            DEFAULT_PAGE_TABLE_FLAGS,
        );
    }

    let pml4_vaddr = Addr::new(PML4_TABLE.get() as usize);
    let mut cr3_raw = RawEntry::default();


    unsafe {
        EntryRef::init(
            &mut cr3_raw,
            Level::CR3,
            KernelImageSpace::v2p(pml4_vaddr),
            Flags::empty(),
        )
    };

    DEFAULT_KERNEL_MAP.call_once(|| X86_64MemoryMap {
        cr3: spin::Mutex::new(cr3_raw),
    });

    // SAFETY: hand rolled cr3 is safe.
    unsafe { set_cr3(cr3_raw) };
    // SAFETY: DEFAULT_KERNEL_MAP is initialized before.
    let memory_manager = X86_64MemoryManager(MapRef::from(unsafe {
        DEFAULT_KERNEL_MAP.get_unchecked()
    }));
    memory_manager
}

pub struct X86_64MemoryManager(MapRef<X86_64MemoryMap>);

impl MemoryManager for X86_64MemoryManager {
    type Map = X86_64MemoryMap;

    fn swap(&mut self, new: MapRef<X86_64MemoryMap>) {
        self.0 = new;
        // SAFETY: MemoryMap is always in valid state.
        unsafe { set_cr3(*self.0.cr3.lock()) };
        self.flush();
    }

    fn map(&mut self) -> &X86_64MemoryMap { &self.0 }

    fn flush(&mut self) { flush_tlb(); }
}

/// # Safety
///
/// Caller need to guarentee entry is a valid cr3 entry that points to a valid
/// paging structure.
unsafe fn set_cr3(entry: RawEntry) {
    // SAFETY: see function safety.
    unsafe { asm!("mov cr3, {}", in(reg) entry.0) };
}
fn cr3() -> RawEntry {
    let out: usize;
    // SAFETY: reading from control register is safe.
    unsafe { asm!("mov {}, cr3", out(reg) out) };
    RawEntry(out)
}
fn flush_tlb() {
    // TODO: use invlpg instead

    // SAFETY: reading then write to cr3 from current core is safe.
    unsafe {
        asm!(
            "mov {tmp}, cr3",
            "mov cr3, {tmp}",
            tmp = out(reg) _
        );
    }
}

// FIXME: When map operations preempt each other, multiple mutable references
// to kernel page table may exist at the same time.

/// A memory mapping that is represented by a cr3 entry.
///
/// The cr3 entry points to a PML4 table, which holds both kernel and userspace
/// mapping. Kernel mapping is shared across all [`X86_64MemoryMap`]s, and is
/// not dropped when [`X86_64MemoryMap`] is dropped.
pub struct X86_64MemoryMap {
    cr3: spin::Mutex<RawEntry>,
}
impl X86_64MemoryMap {
    /// Call a continuation closure with the underlying cr3 `EntryRef` while
    /// locked.
    fn using_entry_ref<F, T>(&self, cont: F) -> T
    where
        F: FnOnce(EntryRef<'_>) -> T,
    {
        let mut cr3 = self.cr3.lock();
        // SAFETY: cr3 holds a top-level cr3 entry.
        let entry_ref = unsafe { EntryRef::from_raw(&mut cr3, Level::CR3) };
        cont(entry_ref)
    }

    /// Call a continuation closure with the underlying top-level `TableRef`
    /// while locked.
    fn using_table_ref<F, T>(&self, cont: F) -> T
    where
        F: FnOnce(TableRef<'_>) -> T,
    {
        self.using_entry_ref(|entry_ref| {
            let EntryTarget::Table(level, addr) = entry_ref.target() else {
                unreachable!()
            };
            let table_vaddr = PhysicalRemapSpace::p2v(addr);
            // SAFETY: cr3 contains valid pointer to the top-level table. We maintain
            // ownership over that table.
            let raw_table = unsafe { table_vaddr.into_ptr::<RawTable>().as_mut_unchecked() };
            // SAFETY: entry_ref provides the correct table level.
            let table_ref = unsafe { TableRef::from_raw(level, raw_table) };
            cont(table_ref)
        })
    }
}

/// Allocate a page table. Returns both virtual and physical addresses.
fn allocate_table() -> (Addr<impl VirtSpace>, Addr<UMASpace>) {
    let table_ptr = PageAllocator
        .allocate_zeroed(Layout::new::<RawTable>())
        .expect("Allocation failed!");
    let table_vaddr: Addr<PhysicalRemapSpace> =
        Addr::new(table_ptr.cast::<RawTable>().as_ptr() as usize);
    let table_paddr = PhysicalRemapSpace::v2p(table_vaddr);
    (table_vaddr, table_paddr)
}
/// Deallocate a page table.
unsafe fn deallocate_table(ptr: *mut RawTable) {
    let vaddr = Addr::from_mut_ptr(ptr);
    let paddr = PhysicalRemapSpace::v2p(vaddr);
    unsafe {
        addr::Allocator::deallocate(
            &PageAllocator,
            paddr,
            Layout::new::<RawTable>(),
        )
    }
}

impl MemoryMap for X86_64MemoryMap {
    fn new() -> Self {
        let (table_vaddr, table_paddr) = allocate_table();

        // SAFETY: this table will be the top-level table after initialization.
        let pml4_table_ref = unsafe {
            TableRef::from_raw(
                Level::PML4,
                table_vaddr.into_ptr::<RawTable>().as_mut_unchecked(),
            )
        };

        // Copy over kernel page tables from the default map.
        let cur_map = DEFAULT_KERNEL_MAP.get().unwrap();

        cur_map.using_table_ref(move |table_ref: TableRef| {
            pml4_table_ref.raw().0[KERNEL_TABLES_CNT..]
                .copy_from_slice(&table_ref.raw().0[KERNEL_TABLES_CNT..]);
        });
        let mut cr3 = RawEntry::default();
        // SAFETY: We hold cr3.
        unsafe {
            EntryRef::init(
                &mut cr3,
                Level::CR3,
                table_paddr,
                Flags::empty(),
            )
        };
        X86_64MemoryMap {
            cr3: spin::Mutex::new(cr3),
        }
    }

    unsafe fn map<V: VirtSpace, const N: usize>(
        &self,
        vpage: Page<V>,
        ppage: Page<UMASpace>,
        flags: Flags,
        allocator: &mut impl addr::Allocator<UMASpace>,
    ) -> Option<()> {
        debug_assert!(vpage.page_size() == ppage.page_size());
        let mut _kernel_map_guard = None;
        if V::IS_KERNEL {
            _kernel_map_guard = Some(KERNEL_MAP_LOCK.lock());
        }

        self.using_entry_ref(|cr3| {
            // SAFETY: allocate_table uses physical remap space.
            let mut walker = unsafe { LinearWalker::new(cr3, vpage.start()) };

            let mut cur_level = walker.cur().level();
            let target_level = Level::from_page_size(vpage.page_size());

            while cur_level != target_level {
                walker.down(allocator);
                cur_level = walker.cur().level();
            }

            walker.cur().reinit(ppage.start(), flags);
        });
        drop(_kernel_map_guard);
        Some(())
    }

    unsafe fn unmap<V: VirtSpace>(&self, vaddr: Addr<V>) { todo!() }

    fn translate<V: VirtSpace>(&self, vaddr: Addr<V>) -> Option<Addr<UMASpace>> {
        let mut _kernel_map_guard = None;
        if V::IS_KERNEL {
            _kernel_map_guard = Some(KERNEL_MAP_LOCK.lock());
        }

        let ret = self.using_entry_ref(|cr3| {
            // SAFETY: allocate_table uses physical remap space.
            let mut walker = unsafe { LinearWalker::new(cr3, vaddr) };

            while walker.try_down().is_some() {}

            match walker.cur().target() {
                EntryTarget::None => None,
                EntryTarget::Page(_, addr) => Some(addr),
                EntryTarget::Table(..) => unreachable!(),
            }
        });
        drop(_kernel_map_guard);
        ret
    }
}
impl Drop for X86_64MemoryMap {
    fn drop(&mut self) {
        // NOTE: Dont call this on kernel page!
        fn drop_entry_target(ent: EntryRef<'_>) {
            let EntryTarget::Table(level, addr) = ent.target() else {
                return;
            };
            let table_ptr = PhysicalRemapSpace::p2v(addr).into_ptr::<RawTable>();
            // SAFETY: entry_ref provides a valid physical addr to table, which is mapped in
            // PhysicalRemapSpace.
            let raw_table = unsafe { table_ptr.as_mut_unchecked() };
            // SAFETY: entry_ref provides the correct table level.
            let table = unsafe { TableRef::from_raw(level, raw_table) };
            for entry in table.entry_refs() {
                drop_entry_target(entry)
            }
            unsafe {
                deallocate_table(table_ptr);
            };
        }

        self.using_table_ref(|mut pml4_table| {
            for entry in pml4_table
                .reborrow()
                .entry_refs()
                .into_iter()
                .take(KERNEL_TABLES_CNT)
            {
                drop_entry_target(entry);
            }
            unsafe { deallocate_table(pml4_table.raw() as *mut RawTable) };
        })
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
    fn down(&mut self, alloc: &mut impl addr::Allocator<UMASpace>) -> &mut EntryRef<'a> {
        if self.cur_entry.level().next_level().is_none() {
            return self.cur();
        }

        let target = self.cur_entry.target();
        match target {
            EntryTarget::None | EntryTarget::Page(..) => {
                let table_paddr = alloc.allocate(PageSize::Small.layout()).unwrap().base;
                let table_level = self.cur_entry.level().next_level().unwrap();
                self.cur_entry.reinit(table_paddr, DEFAULT_PAGE_TABLE_FLAGS);
                unsafe { self.down_with_table(table_paddr, table_level) }
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
