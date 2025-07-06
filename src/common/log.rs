use core::fmt::{Arguments, Write};

pub use mac::*;

use crate::drivers::serial;
use crate::drivers::vga::{Color, VGA_BUFFER};
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
}

pub fn ok(msg: Arguments) { log("OK", Color::Green, msg); }
pub fn error(msg: Arguments) { log("ERROR!", Color::Red, msg); }
pub fn panic(msg: Arguments) { log("PANIC!", Color::Purple, msg); }

fn log(header: &'static str, color: Color, msg: Arguments) {
    // log_vga(header, color, msg);
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
