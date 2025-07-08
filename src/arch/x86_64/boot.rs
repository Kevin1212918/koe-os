use core::arch::global_asm;

global_asm!(include_str!("boot.S"));

pub const MULTIBOOT_ARCHITECTURE: u32 = 0;
