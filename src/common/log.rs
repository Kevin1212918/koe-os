use core::fmt::{Arguments, Write};

pub use mac::*;

use crate::drivers::serial;
use crate::drivers::vga::{Color, VGA_BUFFER};
use crate::interrupt::IntrptGuard;
pub mod mac {
    macro_rules! ok {
        ($($arg:tt)*) => {
            ok(format_args!($($arg)*))
        };
    }
    pub(crate) use ok;
    macro_rules! error {
        ($($arg:tt)*) => {
            error(format_args!($($arg)*))
        };
    }
    pub(crate) use error;
    macro_rules! info {
        ($($arg:tt)*) => {
            info(format_args!($($arg)*))
        };
    }
    pub(crate) use info;
}

pub fn ok(msg: Arguments) { log("OK", Color::Green, msg); }
pub fn info(msg: Arguments) { log("INFO", Color::Blue, msg); }
pub fn error(msg: Arguments) { log("ERROR!", Color::Red, msg); }
pub fn panic(msg: Arguments) { log("PANIC!", Color::Purple, msg); }



fn log(header: &'static str, color: Color, msg: Arguments) {
    let _intrpt = IntrptGuard::new();
    log_vga(header, color, msg);
    log_serial(header, color, msg);
}

fn log_vga(header: &'static str, color: Color, msg: Arguments) {
    let mut sink = VGA_BUFFER.lock();

    sink.set_color(Color::Gray, Color::Black, true);
    write!(sink, "[");
    sink.set_color(color, Color::Black, true);
    write!(sink, "{:^6}", header);
    sink.set_color(Color::Gray, Color::Black, true);
    write!(sink, "] {}\n", msg);
}
fn log_serial(header: &'static str, color: Color, msg: Arguments) {
    let mut sink = serial::COM1.lock();

    write!(sink, "[");
    write!(sink, "{:^6}", header);
    write!(sink, "] {}\n", msg);
}
