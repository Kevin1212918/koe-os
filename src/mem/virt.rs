//!
//! 
//! # Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0xFFFF888000000000:0xFFFFC88000000000|Physical Memory Remap      | 64 TB |
//! |0xFFFFC90000000000:0xFFFFE90000000000|VAlloc                     | 32 TB |
//! |0xFFFFFE8000000000:0xFFFFFF0000000000|Recursive Paging           | 0.5TB |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data           |       |

use core::{alloc::Layout, marker::PhantomData, ops::{Add, Range}, sync::atomic::AtomicUsize};

use derive_more::derive::{Into, Sub};
use multiboot2::BootInformation;

use crate::mem::{addr::{AddrRange}, phy};

use super::{addr::{Addr, AddrSpace}, addr::{PageAddr, PageManager, PageSize}};

pub trait VirtSpace: AddrSpace {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VAllocSpace;
impl VirtSpace for VAllocSpace {}
impl AddrSpace for VAllocSpace {
    const RANGE: Range<usize> = {
        let start = 0xFFFF_C900_0000_0000;
        let end = 0xFFFF_E900_0000_0000;
        start .. end
    };
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
    const RANGE: Range<usize> = {
        let start = 0xFFFF_FFFF_8000_0000;
        let end = 0xFFFF_FFFF_FF60_0000;
        start .. end
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalRemapSpace;
impl PhysicalRemapSpace {
    pub const OFFSET: usize = Self::RANGE.start;
}
impl VirtSpace for PhysicalRemapSpace {}
impl AddrSpace for PhysicalRemapSpace {
    const RANGE: Range<usize> = {
        let start = 0xFFFF_8880_0000_0000;
        let end = 0xFFFF_C880_0000_0000;
        start .. end
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RecursivePagingSpace;
impl VirtSpace for RecursivePagingSpace {}
impl AddrSpace for RecursivePagingSpace {
    const RANGE: Range<usize> = {
        let start = 0xFFFF_FE80_0000_0000;
        let end = 0xFFFF_FF00_0000_0000;
        start .. end
    };
}


pub struct BumpMemoryManager<S: VirtSpace> {
    brk: AtomicUsize,
    _space: PhantomData<S>,
}
impl<S: VirtSpace> BumpMemoryManager<S> {
    pub fn allocate(&self, layout: Layout) -> Option<Addr<S>> {
        use core::sync::atomic::Ordering;
        loop {
            let old_brk = self.brk.load(Ordering::Relaxed);
            let new_brk = old_brk.checked_next_multiple_of(layout.align())?;

            if (S::RANGE.end - new_brk) < layout.size() {
                return None;
            } 
            let new_brk = new_brk + layout.size();

            let res = self.brk.compare_exchange_weak(
                old_brk, 
                new_brk, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            );
            if res.is_ok() {
                return Some(Addr::new(new_brk));
            }
        }
    }
}
impl<S: VirtSpace> PageManager<S> for BumpMemoryManager<S> {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageAddr<S>> {
        use core::sync::atomic::Ordering;

        let size = cnt.checked_mul(page_size.usize())?;
        let align = page_size.alignment();
        loop {
            let old_brk = self.brk.load(Ordering::Relaxed);
            let new_brk = old_brk.checked_next_multiple_of(align)?;

            if (S::RANGE.end - new_brk) < size {
                return None;
            } 
            let new_brk = new_brk + size;

            let res = self.brk.compare_exchange_weak(
                old_brk, 
                new_brk, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            );
            if res.is_ok() {
                return Some(PageAddr::new(Addr::new(new_brk), page_size))
            }
        }

    }
    
    fn allocate_pages_at(&self, _: usize, _: PageSize, _: PageAddr<S>) -> Option<PageAddr<S>> {
        panic!("BrkAllocator does not implement allocate_at");
    }

    unsafe fn deallocate_pages(&self, addr: PageAddr<S>, cnt: usize) {
        panic!("BumpMemoryManager does not implement deallocate");
    }
    
}

