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