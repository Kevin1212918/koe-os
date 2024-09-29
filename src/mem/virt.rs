//!
//! 
//! # Virtual Memory Layout
//! | Address                             | Description                       |
//! |:------------------------------------|----------------------------------:|
//! |0xFFFFF00000000000:0xFFFFF10000000000|VirtAlloc                          |
//! |0xFFFFFFFF40000000:0xFFFFFFFF80000000|TempAlloc                          |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data                   |

use derive_more::derive::{From, Into, Sub};

use super::AddrRange;

/// Address in virtual address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Into, From, PartialEq, Eq, PartialOrd, Ord, Hash, Sub)]
pub struct VAddr(usize);    
impl VAddr {
    pub fn add_offset(mut self, x: usize) -> Self {
        self.0 += x;
        self
    }
    pub fn sub_offset(mut self, x: usize) -> Self {
        self.0 -= x;
        self
    }
}

#[inline]
#[allow(non_snake_case)]
pub const fn VIRT_ALLOC_RANGE() -> AddrRange {
    AddrRange {
        start: 0xFFFF_F000_0000_0000, 
        end: 0xFFFF_F100_0000_0000
    }
}

#[inline]
#[allow(non_snake_case)]
pub const fn TEMP_ALLOC_RANGE() -> AddrRange {
    AddrRange { 
        start: 0xFFFF_FFFFF_4000_0000, 
        end: 0xFFFF_FFFF_8000_0000 
    }
}

#[inline]
#[allow(non_snake_case)]
pub const fn MAX_KERNEL_RANGE() -> AddrRange {
    AddrRange { 
        start: 0xFFFF_FFFFF_8000_0000, 
        end: 0xFFFF_FFFF_FF60_0000
    }
}

pub enum VirtMemTyp {
    KernelData,
}
pub struct VirtAllocator {
    kernel_data_brk: VAddr,
}
impl VirtAllocator {
    fn new() -> Self {
        VirtAllocator { 
            kernel_data_brk: VAddr::from(VIRT_ALLOC_RANGE().start) 
        }
    }
    fn allocate(&mut self, typ: VirtMemTyp, size: usize) -> Option<VAddr> {
        match typ {
            VirtMemTyp::KernelData => {
                let new_brk = self.kernel_data_brk.add_offset(size);
                let max = VAddr::from(VIRT_ALLOC_RANGE().end);
                if new_brk > max {
                    None
                } else {
                    let ret = Some(self.kernel_data_brk);
                    self.kernel_data_brk = new_brk;
                    ret
                }

            },
        }
    }
    unsafe fn deallocate(&mut self, typ: VirtMemTyp, size: usize) {
        match typ {
            VirtMemTyp::KernelData => {
                self.kernel_data_brk = self.kernel_data_brk.sub_offset(size);
            }
        }
    }
}
