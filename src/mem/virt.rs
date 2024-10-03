//!
//! 
//! # Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0xFFFF888000000000:0xFFFFC88000000000|Physical Memory Map        | 64TiB |
//! |0xFFFFF00000000000:0xFFFFF10000000000|VirtAlloc                  | 1TiB  |
//! |0xFFFFF10000000000:0xFFFFF20000000000|IOMap                      | 1TiB  |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data           |       |

use derive_more::derive::{Into, Sub};

use super::addr::{VAddr, VRange};

pub const PHYSICAL_MAP_OFFSET: usize = PHYSICAL_MAP_RANGE.start.into_usize();
pub const PHYSICAL_MAP_RANGE: VRange = {
    let start = unsafe{VAddr::from_usize(0xFFFF_8880_0000_0000)};
    let end = unsafe{VAddr::from_usize(0xFFFF_C880_0000_0000)};
    start .. end
};
pub const VIRT_ALLOC_RANGE: VRange = {
    let start = unsafe{VAddr::from_usize(0xFFFF_F000_0000_0000)};
    let end = unsafe{VAddr::from_usize(0xFFFF_F100_0000_0000)};
    start .. end
};

pub const IO_MAP_RANGE: VRange = {
    let start = unsafe{VAddr::from_usize(0xFFFF_F100_0000_0000)};
    let end = unsafe{VAddr::from_usize(0xFFFF_F200_0000_0000)};
    start .. end
};

pub const MAX_KERNEL_RANGE: VRange = {
    let start = unsafe{VAddr::from_usize(0xFFFF_FFFF_8000_0000)};
    let end = unsafe{VAddr::from_usize(0xFFFF_FFFF_FF60_0000)};
    start .. end
};

pub enum VirtMemTyp {
    KernelData,
}
pub struct VirtAllocator {
    kernel_data_brk: VAddr,
}
impl VirtAllocator {
    fn new() -> Self {
        VirtAllocator { 
            kernel_data_brk: VIRT_ALLOC_RANGE.start
        }
    }
    fn allocate(&mut self, typ: VirtMemTyp, size: usize) -> Option<VAddr> {
        match typ {
            VirtMemTyp::KernelData => {
                let new_brk = self.kernel_data_brk.byte_add(size);
                let max = VIRT_ALLOC_RANGE.end;
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
                self.kernel_data_brk = self.kernel_data_brk.byte_sub(size);
            }
        }
    }
}
