use core::fmt::Write as _;
use core::ops::Deref;

use ::alloc::vec::Vec;
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

use crate::common::hlt;
use crate::drivers::vga::VGA_BUFFER;
use crate::log;

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



    // FIXME: reorganize test cases
    let mut test = Vec::new();
    for i in 0..1200 {
        test.push(i);
    }
    let mut test2: Vec<u32> = Vec::new();
    for i in 0..1200 {
        test2.push(i);
    }
    for i in test.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    drop(test);
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    let mut test3 = Vec::new();
    for j in 0..1200 {
        let mut inner = Vec::new();
        for i in 0..10 {
            inner.push(i * j);
        }
        test3.push(inner);
    }
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    for (j, list) in test3.iter().enumerate() {
        for (i, num) in list.iter().enumerate() {
            assert!(*num as usize == i * j);
        }
    }
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
