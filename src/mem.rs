use core::arch::asm;
use core::ops::Range;

use addr::{Addr, AddrSpace, PageAddr};
use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use multiboot2::BootInformation;
use paging::{Flags, MemoryManager, MMU};
use virt::KernelImageSpace;


pub mod addr;
mod alloc;
mod paging;
mod phy;
mod virt;

pub use alloc::{GlobalAllocator, PageAllocator};

pub use paging::{X86_64MemoryManager, X86_64MemoryMap};
pub use phy::UMASpace;
pub use virt::PhysicalRemapSpace;

use crate::common::{hlt, Privilege};

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;


extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}

/// Initialize paging and global/page allocators. Several memory regions are
/// leaked here.
pub fn init(boot_info: BootInformation) {
    init_gdtr();
    let bmm = phy::init_boot_mem(&boot_info);
    MMU.call_once(|| X86_64MemoryManager::init(&bmm));
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

// ------------ Segmentation stuff -------------

fn init_gdtr() {
    unsafe { GDT.0[1] = SegmentDesc::code() };

    let gdtr = Gdtr {
        limit: (Gdt::LEN * size_of::<SegmentDesc>() - 1) as u16,
        base: &raw const GDT,
    };

    unsafe {
        asm!(
            "lgdt [{gdtr}]",
            gdtr = in(reg) &gdtr as *const Gdtr
        )
    };
}

#[repr(C, packed(2))]
struct Gdtr {
    limit: u16,
    base: *const Gdt,
}

static mut GDT: Gdt = Gdt([const { SegmentDesc::invalid() }; Gdt::LEN]);

#[repr(C, align(8))]
struct Gdt([SegmentDesc; Self::LEN]);
impl Gdt {
    const LEN: usize = 2;
}
#[repr(C, packed)]
struct SegmentDesc(u64);
impl SegmentDesc {
    const DEFAULT_IDXS: Range<usize> = 54..55;
    const DESC_TYPE_IDXS: Range<usize> = 44..45;
    const DPL_IDXS: Range<usize> = 45..47;
    const GRANULARITY_IDXS: Range<usize> = 55..56;
    const LONG_MODE_IDXS: Range<usize> = 53..54;
    const P_IDXS: Range<usize> = 47..48;
    const TYPE_IDXS: Range<usize> = 40..44;

    fn code() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::TYPE_IDXS].store_le(0b1000);
        view[Self::DESC_TYPE_IDXS].store_le(1);
        view[Self::DPL_IDXS].store_le(Privilege::Kernel as u8);
        view[Self::P_IDXS].store_le(1);
        view[Self::LONG_MODE_IDXS].store(1);
        Self(bits)
    }

    const fn invalid() -> Self { Self(0) }
}
