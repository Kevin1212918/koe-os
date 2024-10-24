use core::arch::global_asm;

mod multiboot2_header;
mod memblock;

pub use memblock::{MemblockAllocator, MemblockAllocatorBuilder};

global_asm!(include_str!("boot/boot.S"));