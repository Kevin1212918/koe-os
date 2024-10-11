use core::ops::{BitAnd, BitOr, Range, Sub};

use addr::{PAddr, VAddr};
use multiboot2::BootInformation;
use virt::{PhysicalRemapSpace, VirtSpace};

use crate::drivers::vga::VGA_BUFFER;
use core::fmt::Write as _;

mod phy;
mod virt;
mod alloc;
mod paging;
mod addr;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}

/// Initialize boot time paging, allocator, as well as parse `mbi_ptr` into
/// `BootInformation`
pub fn init<'boot>(mbi_ptr: usize) -> BootInformation<'boot> {
    let boot_info = paging::init(mbi_ptr);
    write!(VGA_BUFFER.lock(), "paging initalized\n").expect("VGA text mode not available");
    phy::init(&boot_info);
    write!(VGA_BUFFER.lock(), "phy initalized\n").expect("VGA text mode not available");
    boot_info
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
    unsafe { PAddr::from_usize(addr.into_usize() - KERNEL_OFFSET_VMA) }
}
pub fn phy_to_virt(addr: PAddr) -> VAddr {
    let vaddr = addr.byte_add(PhysicalRemapSpace::OFFSET).into_usize();
    unsafe { VAddr::from_usize(vaddr) }
}