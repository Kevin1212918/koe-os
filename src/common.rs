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
pub fn hlt() -> ! {
    loop {
        unsafe { asm!("hlt") };
    }
}

pub mod array_forest;
pub mod ll;
pub mod panic;

#[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
pub mod pmio;

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        write!(VGA_BUFFER.lock(), $($arg)*).ok()
    };
}

#[repr(u8)]
pub enum Privilege {
    User = 3,
    Kernel = 0,
}
