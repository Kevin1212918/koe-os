
#[allow(non_upper_case_globals)]
pub const KiB: usize = 1 << 10;
#[allow(non_upper_case_globals)]
pub const MiB: usize = 1 << 20;
#[allow(non_upper_case_globals)]
pub const GiB: usize = 1 << 30;
#[allow(non_upper_case_globals)]
pub const TiB: usize = 1 << 40;

pub type InstrPtr = usize;
pub type StackPtr = usize;

pub mod array_forest;
pub mod ll;
pub mod log;
pub mod panic;

#[repr(u8)]
pub enum Privilege {
    User = 3,
    Kernel = 0,
}
