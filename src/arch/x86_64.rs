use core::arch::asm;

use crate::common::StackPtr;

pub mod boot;
mod gdt;
pub mod pmio;

#[inline(always)]
pub fn die() -> ! {
    // SAFETY: hlt is safe
    unsafe { asm!("cli", "hlt") };
    unreachable!()
}

#[inline(always)]
pub fn hlt() {
    // SAFETY: hlt is safe
    unsafe { asm!("hlt") };
}

pub fn stack_ptr() -> StackPtr {
    let stack_ptr: usize;

    // SAFETY: Reading stack pointer is safe.
    unsafe { asm!("mov r11, rsp", out("r11") stack_ptr ) };
    debug_assert!(stack_ptr != 0);
    stack_ptr
}
