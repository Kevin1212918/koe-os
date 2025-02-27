//!
//!
//! # Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0xFFFF888000000000:0xFFFFC88000000000|Physical Memory Remap      | 64 TB |
//! |0xFFFFC90000000000:0xFFFFE90000000000|Kernel Heap                | 32 TB |
//! |0xFFFFFE8000000000:0xFFFFFF0000000000|Recursive Paging           | 0.5TB |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data           |       |

use alloc::collections::btree_set::BTreeSet;
use core::ops::Range;
use core::sync::atomic::AtomicUsize;

use super::addr::{Addr, AddrSpace, PageRange, PageSize};
use super::LinearSpace;
use crate::mem::addr::AddrRange;
use crate::mem::phy;

pub trait VirtSpace: AddrSpace {}


/// Marks the lowest bound of static data on heap.
static HEAP_LOW_MARK: AtomicUsize = AtomicUsize::new(0xFFFF_E900_0000_0000);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KernelHeapSpace;
impl KernelHeapSpace {
    fn low_mark() -> &'static AtomicUsize { &HEAP_LOW_MARK }
}
impl VirtSpace for KernelHeapSpace {}
impl AddrSpace for KernelHeapSpace {
    const RANGE: Range<usize> = 0xFFFF_C900_0000_0000..0xFFFF_E900_0000_0000;
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KernelSpace;
impl KernelSpace {
    pub fn v2p(vaddr: Addr<Self>) -> Addr<phy::LinearSpace> {
        assert!(Self::RANGE.contains(&vaddr.usize()));
        Addr::new(vaddr.usize() - Self::RANGE.start)
    }
}
impl VirtSpace for KernelSpace {}
impl AddrSpace for KernelSpace {
    const RANGE: Range<usize> = 0xFFFF_FFFF_8000_0000..0xFFFF_FFFF_FF60_0000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalRemapSpace;
impl PhysicalRemapSpace {
    pub const OFFSET: usize = Self::RANGE.start;

    pub const fn p2v(paddr: Addr<LinearSpace>) -> Addr<Self> {
        Addr::new(paddr.usize() + Self::OFFSET)
    }

    pub const fn v2p(vaddr: Addr<Self>) -> Addr<LinearSpace> {
        Addr::new(vaddr.usize() - Self::OFFSET)
    }
}
impl VirtSpace for PhysicalRemapSpace {}
impl AddrSpace for PhysicalRemapSpace {
    const RANGE: Range<usize> = 0xFFFF_8880_0000_0000..0xFFFF_C880_0000_0000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecursivePagingSpace;
impl VirtSpace for RecursivePagingSpace {}
impl AddrSpace for RecursivePagingSpace {
    const RANGE: Range<usize> = 0xFFFF_FE80_0000_0000..0xFFFF_FF00_0000_0000;
}

struct VirtMemoryArea<S: VirtSpace> {
    range: PageRange<S>,
    flag: u8,
}
impl<S: VirtSpace> PartialEq for VirtMemoryArea<S> {
    fn eq(&self, other: &Self) -> bool { self.range.base.addr() == other.range.base.addr() }
}
impl<S: VirtSpace> Eq for VirtMemoryArea<S> {}
impl<S: VirtSpace> PartialOrd for VirtMemoryArea<S> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.range.base.addr().partial_cmp(&other.range.base.addr())
    }
}
impl<S: VirtSpace> Ord for VirtMemoryArea<S> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.range.base.addr().cmp(&other.range.base.addr())
    }
}

