use core::arch::asm;

pub mod boot;
pub mod interrupt;
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
