use core::arch::asm;

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
