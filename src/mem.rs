use core::ops::{Add, BitAnd, BitOr, Range, Sub};

use addr::Addr;
use memblock::BootMemoryManager;
use multiboot2::BootInformation;
use addr::{PageAddr, PageSize, PageManager};
use paging::{Flag, MemoryManager, X86_64MemoryManager};
use virt::{KernelSpace, PhysicalRemapSpace, VAllocSpace, VirtSpace};

use crate::{common::hlt, drivers::vga::VGA_BUFFER};
use core::fmt::Write as _;

mod phy;
mod virt;
mod alloc;
mod paging;
pub mod memblock;
pub mod addr;

pub use phy::LinearSpace;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}

/// Initialize boot time paging, allocator, as well as parse `mbi_ptr` into
/// `BootInformation`
pub fn init(boot_info: BootInformation<'_>, boot_alloc: &BootMemoryManager) {
    let mem_man = unsafe {X86_64MemoryManager::init(boot_alloc)};
}

#[inline]
pub const fn kernel_offset_vma() -> usize {
    KERNEL_OFFSET_VMA
}
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
    unsafe {
        Addr::new(&_KERNEL_START_LMA as *const u8 as usize)
    }
}
#[inline]
pub fn kernel_end_lma() -> Addr<LinearSpace> {
    kernel_start_lma().byte_add(kernel_size())
}
#[inline]
pub fn kernel_size() -> usize {
    kernel_end_vma().addr_sub(kernel_start_vma()).try_into()
        .expect("kernel_end_vma should be larger than kernel_start_vma")
}
pub unsafe fn kernel_v2p(addr: Addr<KernelSpace>) -> Addr<LinearSpace> {
    Addr::new(addr.usize() - KERNEL_OFFSET_VMA)
}
pub fn p2v(addr: Addr<LinearSpace>) -> Addr<PhysicalRemapSpace> {
    let vaddr = addr.byte_add(PhysicalRemapSpace::OFFSET).usize();
    Addr::new(vaddr)
}