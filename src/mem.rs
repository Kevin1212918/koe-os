use core::ops::Deref;

use addr::Addr;
use multiboot2::BootInformation;
use paging::MMU;
use virt::{KernelSpace, PhysicalRemapSpace};


mod addr;
mod alloc;
mod paging;
mod phy;
mod virt;

pub use phy::LinearSpace;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}

/// Initialize boot time paging, allocator, as well as parse `BootInformation`
pub fn init(boot_info: BootInformation) {
    let memory_info = boot_info
        .memory_map_tag()
        .expect("Currently does not support uefi memory map");
    phy::init(memory_info.memory_areas());
}

#[inline]
pub const fn kernel_offset_vma() -> usize { KERNEL_OFFSET_VMA }
#[inline]
pub fn kernel_start_vma() -> Addr<KernelSpace> {
    // SAFETY: _KERNEL_START_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    Addr::from_ref(unsafe { &_KERNEL_START_VMA })
}
#[inline]
pub fn kernel_end_vma() -> Addr<KernelSpace> {
    // SAFETY: _KERNEL_END_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    Addr::from_ref(unsafe { &_KERNEL_END_VMA })
}
#[inline]
pub fn kernel_start_lma() -> Addr<LinearSpace> {
    // SAFETY: _KERNEL_START_LMA is on symbol table created by linker. The
    // address of the symbol is the load memory address of kernel, which
    // should be loaded during real mode at the actual physical address
    unsafe { Addr::new(&_KERNEL_START_LMA as *const u8 as usize) }
}
#[inline]
pub fn kernel_end_lma() -> Addr<LinearSpace> { kernel_start_lma().byte_add(kernel_size()) }
#[inline]
pub fn kernel_size() -> usize {
    kernel_end_vma()
        .addr_sub(kernel_start_vma())
        .try_into()
        .expect("kernel_end_vma should be larger than kernel_start_vma")
}
pub unsafe fn kernel_v2p(addr: Addr<KernelSpace>) -> Addr<LinearSpace> {
    Addr::new(addr.usize() - KERNEL_OFFSET_VMA)
}
pub fn p2v(addr: Addr<LinearSpace>) -> Addr<PhysicalRemapSpace> {
    let vaddr = addr.byte_add(PhysicalRemapSpace::OFFSET).usize();
    Addr::new(vaddr)
}
