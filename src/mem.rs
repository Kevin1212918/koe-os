use core::ops::{BitAnd, BitOr, Range, Sub};

use addr::{PAddr, VAddr};
use virt::PHYSICAL_MAP_OFFSET;

mod phy;
mod virt;
mod alloc;
mod page;
mod addr;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}
#[inline]
pub const fn kernel_offset_vma() -> usize {
    KERNEL_OFFSET_VMA
}
#[inline]
pub fn kernel_start_vma() -> VAddr {
    // SAFETY: _KERNEL_START_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    VAddr::from_ref(unsafe { &_KERNEL_START_VMA })
}
#[inline]
pub fn kernel_end_vma() -> VAddr {
    // SAFETY: _KERNEL_END_VMA is on symbol table created by linker. The
    // address of the symbol is the virtual memory address of kernel.
    VAddr::from_ref(unsafe { &_KERNEL_END_VMA })
}
#[inline]
pub fn kernel_start_lma() -> PAddr {
    // SAFETY: _KERNEL_START_LMA is on symbol table created by linker. The
    // address of the symbol is the load memory address of kernel, which 
    // should be loaded during real mode at the actual physical address
    unsafe {
        PAddr::from_usize(&_KERNEL_START_LMA as *const u8 as usize)
    }
}
#[inline]
pub fn kernel_end_lma() -> PAddr {
    kernel_start_lma().byte_add(kernel_size())
}
#[inline]
pub fn kernel_size() -> usize {
    kernel_end_vma().addr_sub(kernel_start_vma()).try_into()
        .expect("kernel_end_vma should be larger than kernel_start_vma")
}
pub unsafe fn kernel_virt_to_phy(addr: VAddr) -> PAddr {
    unsafe { PAddr::from_usize(addr.into_usize()) }
}
pub fn phy_to_virt(addr: PAddr) -> VAddr {
    unsafe { VAddr::from_usize(addr.byte_add(PHYSICAL_MAP_OFFSET).into_usize()) }
}