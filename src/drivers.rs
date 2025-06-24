pub mod ps2;
pub mod serial;
pub mod vga;

pub fn init() { ps2::init(); }
