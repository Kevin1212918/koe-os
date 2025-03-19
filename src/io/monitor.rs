use core::fmt::Write as _;

use super::keyboard::keycode::*;
use super::keyboard::{KeyEvent, Keyboard, Modifier};
use crate::common::hlt;
use crate::drivers::vga::VGA_BUFFER;

pub struct Monitor<'kb> {
    keyboard: &'kb mut dyn Keyboard,
}
impl<'kb> Monitor<'kb> {
    pub fn new(kb: &'kb mut dyn Keyboard) -> Self { Self { keyboard: kb } }
    pub fn start(&mut self) {
        let mut console = VGA_BUFFER.lock();
        loop {
            let ke = self.keyboard.next();
            let ascii = ke.and_then(ketoa);
            let Some(ascii) = ascii else {
                continue;
            };
            console.write_u8(ascii);
        }
    }
}

fn ketoa(ke: KeyEvent) -> Option<u8> {
    if !ke.is_press {
        return None;
    }

    let is_cap = ke.modifier.contains(Modifier::CAPSLOCK) ^ ke.modifier.contains(Modifier::SHIFT);
    let cap_offset = 32 * is_cap as u8;
    match ke.key {
        KEY_0 => Some(b'0'),
        KEY_1..=KEY_9 => Some(b'1' + (ke.key - KEY_1) as u8),

        KEY_A => Some(b'a' - cap_offset),
        KEY_B => Some(b'b' - cap_offset),
        KEY_C => Some(b'c' - cap_offset),
        KEY_D => Some(b'd' - cap_offset),
        KEY_E => Some(b'e' - cap_offset),
        KEY_F => Some(b'f' - cap_offset),
        KEY_G => Some(b'g' - cap_offset),
        KEY_H => Some(b'h' - cap_offset),
        KEY_I => Some(b'i' - cap_offset),
        KEY_J => Some(b'j' - cap_offset),
        KEY_K => Some(b'k' - cap_offset),
        KEY_L => Some(b'l' - cap_offset),
        KEY_M => Some(b'm' - cap_offset),
        KEY_N => Some(b'n' - cap_offset),
        KEY_O => Some(b'o' - cap_offset),
        KEY_P => Some(b'p' - cap_offset),
        KEY_Q => Some(b'q' - cap_offset),
        KEY_R => Some(b'r' - cap_offset),
        KEY_S => Some(b's' - cap_offset),
        KEY_T => Some(b't' - cap_offset),
        KEY_U => Some(b'u' - cap_offset),
        KEY_V => Some(b'v' - cap_offset),
        KEY_W => Some(b'w' - cap_offset),
        KEY_X => Some(b'x' - cap_offset),
        KEY_Y => Some(b'y' - cap_offset),
        KEY_Z => Some(b'z' - cap_offset),

        KEY_ENTER => Some(b'\n'),
        KEY_SPACE => Some(b' '),
        KEY_BACKSPACE => Some(0x8),

        _ => None,
    }
}
