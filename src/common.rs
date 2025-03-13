#[allow(non_upper_case_globals)]
pub const KiB: usize = 1 << 10;
#[allow(non_upper_case_globals)]
pub const MiB: usize = 1 << 20;
#[allow(non_upper_case_globals)]
pub const GiB: usize = 1 << 30;
#[allow(non_upper_case_globals)]
pub const TiB: usize = 1 << 40;

pub fn hlt() -> ! {
    unsafe { core::arch::asm!("hlt") };
    unreachable!()
}

pub mod array_forest;
pub mod ll;
pub mod panic;

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
