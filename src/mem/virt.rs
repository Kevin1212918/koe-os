//!
//!
//! # Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0xFFFF888000000000:0xFFFFC88000000000|Physical Memory Remap      | 64 TB |
//! |0xFFFFC90000000000:0xFFFFE90000000000|Data Stack                 | 32 TB |
//! |0xFFFFFE8000000000:0xFFFFFF0000000000|Recursive Paging           | 0.5TB |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data           |       |

use core::ops::Range;
use core::sync::atomic::AtomicUsize;

use super::addr::{Addr, AddrSpace, PageRange};
use super::UMASpace;
use crate::mem::phy;

pub trait VirtSpace: AddrSpace {
    fn is_kernel_space() -> bool { Self::RANGE.start >= 0xFFFF_8000_0000_0000 }
}
pub trait KernelSpace: VirtSpace {}
impl<S: KernelSpace> VirtSpace for S {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KernelImageSpace;
impl KernelImageSpace {
    pub fn v2p(vaddr: Addr<Self>) -> Addr<phy::UMASpace> {
        assert!(Self::RANGE.contains(&vaddr.usize()));
        Addr::new(vaddr.usize() - Self::RANGE.start)
    }
}
impl KernelSpace for KernelImageSpace {}
impl AddrSpace for KernelImageSpace {
    const RANGE: Range<usize> = 0xFFFF_FFFF_8000_0000..0xFFFF_FFFF_FF60_0000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalRemapSpace;
impl PhysicalRemapSpace {
    pub const OFFSET: usize = Self::RANGE.start;

    pub const fn p2v(paddr: Addr<UMASpace>) -> Addr<Self> {
        Addr::new(paddr.usize() + Self::OFFSET)
    }

    pub const fn v2p(vaddr: Addr<Self>) -> Addr<UMASpace> {
        Addr::new(vaddr.usize() - Self::OFFSET)
    }
}
impl KernelSpace for PhysicalRemapSpace {}
impl AddrSpace for PhysicalRemapSpace {
    const RANGE: Range<usize> = 0xFFFF_8880_0000_0000..0xFFFF_C880_0000_0000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DataStackSpace;
impl KernelSpace for DataStackSpace {}
impl AddrSpace for DataStackSpace {
    const RANGE: Range<usize> = 0xFFFF_C900_0000_0000..0xFFFF_E900_0000_0000;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecursivePagingSpace;
impl KernelSpace for RecursivePagingSpace {}
impl AddrSpace for RecursivePagingSpace {
    const RANGE: Range<usize> = 0xFFFF_FE80_0000_0000..0xFFFF_FF00_0000_0000;
}
