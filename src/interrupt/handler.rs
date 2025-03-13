use core::fmt::Write as _;

use crate::common::hlt;
use crate::drivers::vga::VGA_BUFFER;
use crate::log;

pub extern "C" fn default_handler() { hlt() }
