use core::arch::asm;
use core::ops::Range;

use addr::Addr;
use arch::MemoryMap;
use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use multiboot2::BootInformation;
use virt::KernelImageSpace;

pub mod addr;
mod alloc;
mod arch;
pub mod paging;
mod phy;
mod virt;

pub use alloc::{GlobalAllocator, PageAllocator, SlabAllocator};

pub use phy::UMASpace;
pub use virt::{PhysicalRemapSpace, UserSpace};

use crate::common::Privilege;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

pub use arch::MMU;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}

/// Initialize paging and global/page allocators. Several memory regions are
/// leaked here.
pub fn init(boot_info: BootInformation) {
    let bmm = phy::init_boot_mem(&boot_info);
    paging::init(&bmm);
    phy::init(bmm);
}


pub const fn kernel_offset_vma() -> usize { KERNEL_OFFSET_VMA }
pub fn kernel_start_vma() -> Addr<KernelImageSpace> {
    // SAFETY: _KERNEL_START_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    Addr::from_ref(unsafe { &_KERNEL_START_VMA })
}
pub fn kernel_end_vma() -> Addr<KernelImageSpace> {
    // SAFETY: _KERNEL_END_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    Addr::from_ref(unsafe { &_KERNEL_END_VMA })
}
pub fn kernel_start_lma() -> Addr<UMASpace> {
    // SAFETY: _KERNEL_START_LMA is on symbol table created by linker. The
    // address of the symbol is the load memory address of kernel, which
    // should be loaded during real mode at the actual physical address
    unsafe { Addr::new(&_KERNEL_START_LMA as *const u8 as usize) }
}
pub fn kernel_end_lma() -> Addr<UMASpace> { kernel_start_lma().byte_add(kernel_size()) }
pub fn kernel_size() -> usize {
    kernel_end_vma()
        .addr_sub(kernel_start_vma())
        .try_into()
        .expect("kernel_end_vma should be larger than kernel_start_vma")
}

pub type Paging = paging::MemoryMapRef<MemoryMap>;
