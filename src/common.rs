use core::arch::asm;

#[allow(non_upper_case_globals)]
pub const KiB: usize = 1 << 10;
#[allow(non_upper_case_globals)]
pub const MiB: usize = 1 << 20;
#[allow(non_upper_case_globals)]
pub const GiB: usize = 1 << 30;
#[allow(non_upper_case_globals)]
pub const TiB: usize = 1 << 40;

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

pub mod array_forest;
pub mod ll;
pub mod log;
pub mod panic;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod pmio;

#[repr(u8)]
pub enum Privilege {
    User = 3,
    Kernel = 0,
}
