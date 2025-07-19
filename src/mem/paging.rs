//! x86-64 4 level ordinary paging

use alloc::sync::Arc;
use core::fmt::Write as _;
use core::ops::Deref;

use bitflags::{bitflags, Flags};
use derive_more::derive::From;

use super::addr::{self, Addr, Page};
use super::phy::BootMemoryManager;
use super::virt::VirtSpace;
use super::UMASpace;

pub fn init(bmm: &BootMemoryManager) { super::arch::init(bmm) }

/// Smart shared reference to a memory map.
#[derive(From)]
pub enum MemoryMapRef<M: MemoryMap> {
    Static(&'static M),
    Arc(Arc<M>),
}
impl<M: MemoryMap> MemoryMapRef<M> {
    pub fn new() -> MemoryMapRef<M> { Self::Arc(Arc::new(MemoryMap::new())) }
}
impl<M: MemoryMap> Deref for MemoryMapRef<M> {
    type Target = M;

    fn deref(&self) -> &Self::Target {
        match self {
            MemoryMapRef::Static(r) => r,
            MemoryMapRef::Arc(r) => r,
        }
    }
}
impl<M: MemoryMap> Clone for MemoryMapRef<M> {
    fn clone(&self) -> Self {
        match self {
            Self::Static(arg0) => Self::Static(arg0),
            Self::Arc(arg0) => Self::Arc(arg0.clone()),
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
    fn swap(&mut self, new: MemoryMapRef<Self::Map>);

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

    /// Maps a virtual page of size `page_size` to `ppage`. Overwrite any
    /// previous virtual page mapping at `vpage`.
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
    unsafe fn map<V: VirtSpace>(
        &self,
        vpage: Page<V>,
        ppage: Page<UMASpace>,
        attr: Attribute,
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

bitflags! {
/// Page attribtutes. Exactly one of the cache flags should be set.
#[derive(Debug, Clone, Copy)]
pub struct Attribute: u16 {
    // Universal
    const IS_USR = 0b1;
    const WRITEABLE = 0b10;

    // Cache.
    const WRITE_BACK = 0b100;
    const UNCACHED = 0b1000;
    const WRITE_COMBINED = 0b1_0000;
    const WRITE_THRU = 0b10_0000;
    const UNCACHED_MINUS = 0b100_0000;
}}

impl Attribute {
    pub fn cache(self) -> Self {
        let cache_flags = Self::WRITE_BACK
            | Self::UNCACHED
            | Self::WRITE_COMBINED
            | Self::WRITE_THRU
            | Self::UNCACHED_MINUS;

        let cache = self.intersection(cache_flags);
        let mut bits = cache.iter();
        let res = bits.next();
        debug_assert!(res.is_some());
        let res = unsafe { res.unwrap_unchecked() };
        debug_assert!(!res.contains_unknown_bits());
        debug_assert!(!res.is_empty());
        debug_assert!(bits.next().is_none());
        res
    }
}
