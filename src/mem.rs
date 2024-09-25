use derive_more::derive::{From, Into};

mod phy;
mod virt;

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
pub fn kernel_start_vma() -> usize {
    unsafe {
        &_KERNEL_START_VMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_end_vma() -> usize {
    unsafe {
        &_KERNEL_END_VMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_start_lma() -> usize {
    unsafe {
        &_KERNEL_START_LMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_end_lma() -> usize {
    kernel_start_lma() + (kernel_end_vma() - kernel_start_vma())
}
#[inline]
pub fn kernel_size() -> usize {
    kernel_end_vma() - kernel_start_vma()
}

pub const USER_START_VMA: usize = 0x0000000000000000;
pub const USER_START_PMA: usize = 0x400000;
