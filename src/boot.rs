use core::arch::global_asm;

mod multiboot2_header;

global_asm!(include_str!("boot/boot.S"));
