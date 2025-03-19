use arraydeque::ArrayDeque;
use bitflags::bitflags;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use keycode::*;

const STATES_LEN: usize = (KEYCODE_MAX + 1).div_ceil(64) as usize;
pub trait Keyboard: Iterator<Item = KeyEvent> {}

pub struct VirtKeyboard {
    states: [u64; STATES_LEN],
    modifier: Modifier,
}

impl VirtKeyboard {
    pub const fn new() -> Self {
        Self {
            states: [0; STATES_LEN],
            modifier: Modifier::empty(),
        }
    }
    pub fn parse(&mut self, packet: (KeyCode, bool)) -> Option<KeyEvent> {
        self.update(packet);
        Some(self.event(packet))
    }
    fn update(&mut self, packet: (KeyCode, bool)) {
        self.states
            .view_bits_mut::<Lsb0>()
            .set(packet.0 as usize, packet.1);
        match packet {
            (KEY_LEFTALT, is_press) => self.modifier.set(Modifier::ALT, is_press),
            (KEY_LEFTCTRL, is_press) => self.modifier.set(Modifier::CTRL, is_press),
            (KEY_LEFTSHIFT, is_press) | (KEY_RIGHTSHIFT, is_press) =>
                self.modifier.set(Modifier::SHIFT, is_press),

            (KEY_CAPSLOCK, true) => self.modifier.toggle(Modifier::CAPSLOCK),
            (KEY_SCROLLLOCK, true) => self.modifier.toggle(Modifier::SCROLLLOCK),
            (KEY_NUMLOCK, true) => self.modifier.toggle(Modifier::NUMLOCK),

            _ => (),
        }
    }
    fn event(&self, packet: (KeyCode, bool)) -> KeyEvent {
        KeyEvent {
            key: packet.0,
            is_press: packet.1,
            modifier: self.modifier,
        }
    }
}

#[derive(Clone, Copy)]
pub struct KeyEvent {
    pub key: KeyCode,
    pub is_press: bool,
    pub modifier: Modifier,
}

bitflags! {
#[derive(Clone, Copy)]
pub struct Modifier: u8 {
    const ALT = 0b1;
    const SHIFT = 0b10;
    const CTRL = 0b100;
    const CAPSLOCK = 0b1000;
    const SCROLLLOCK = 0b10000;
    const NUMLOCK = 0b100000;
}}


pub mod keycode {
    pub type KeyCode = u16;
    // Copied from uapi/linux/input-event-codes.h

    pub const KEY_RESERVED: KeyCode = 0;
    pub const KEY_ESC: KeyCode = 1;
    pub const KEY_1: KeyCode = 2;
    pub const KEY_2: KeyCode = 3;
    pub const KEY_3: KeyCode = 4;
    pub const KEY_4: KeyCode = 5;
    pub const KEY_5: KeyCode = 6;
    pub const KEY_6: KeyCode = 7;
    pub const KEY_7: KeyCode = 8;
    pub const KEY_8: KeyCode = 9;
    pub const KEY_9: KeyCode = 10;
    pub const KEY_0: KeyCode = 11;
    pub const KEY_MINUS: KeyCode = 12;
    pub const KEY_EQUAL: KeyCode = 13;
    pub const KEY_BACKSPACE: KeyCode = 14;
    pub const KEY_TAB: KeyCode = 15;
    pub const KEY_Q: KeyCode = 16;
    pub const KEY_W: KeyCode = 17;
    pub const KEY_E: KeyCode = 18;
    pub const KEY_R: KeyCode = 19;
    pub const KEY_T: KeyCode = 20;
    pub const KEY_Y: KeyCode = 21;
    pub const KEY_U: KeyCode = 22;
    pub const KEY_I: KeyCode = 23;
    pub const KEY_O: KeyCode = 24;
    pub const KEY_P: KeyCode = 25;
    pub const KEY_LEFTBRACE: KeyCode = 26;
    pub const KEY_RIGHTBRACE: KeyCode = 27;
    pub const KEY_ENTER: KeyCode = 28;
    pub const KEY_LEFTCTRL: KeyCode = 29;
    pub const KEY_A: KeyCode = 30;
    pub const KEY_S: KeyCode = 31;
    pub const KEY_D: KeyCode = 32;
    pub const KEY_F: KeyCode = 33;
    pub const KEY_G: KeyCode = 34;
    pub const KEY_H: KeyCode = 35;
    pub const KEY_J: KeyCode = 36;
    pub const KEY_K: KeyCode = 37;
    pub const KEY_L: KeyCode = 38;
    pub const KEY_SEMICOLON: KeyCode = 39;
    pub const KEY_APOSTROPHE: KeyCode = 40;
    pub const KEY_GRAVE: KeyCode = 41;
    pub const KEY_LEFTSHIFT: KeyCode = 42;
    pub const KEY_BACKSLASH: KeyCode = 43;
    pub const KEY_Z: KeyCode = 44;
    pub const KEY_X: KeyCode = 45;
    pub const KEY_C: KeyCode = 46;
    pub const KEY_V: KeyCode = 47;
    pub const KEY_B: KeyCode = 48;
    pub const KEY_N: KeyCode = 49;
    pub const KEY_M: KeyCode = 50;
    pub const KEY_COMMA: KeyCode = 51;
    pub const KEY_DOT: KeyCode = 52;
    pub const KEY_SLASH: KeyCode = 53;
    pub const KEY_RIGHTSHIFT: KeyCode = 54;
    pub const KEY_KPASTERISK: KeyCode = 55;
    pub const KEY_LEFTALT: KeyCode = 56;
    pub const KEY_SPACE: KeyCode = 57;
    pub const KEY_CAPSLOCK: KeyCode = 58;
    pub const KEY_F1: KeyCode = 59;
    pub const KEY_F2: KeyCode = 60;
    pub const KEY_F3: KeyCode = 61;
    pub const KEY_F4: KeyCode = 62;
    pub const KEY_F5: KeyCode = 63;
    pub const KEY_F6: KeyCode = 64;
    pub const KEY_F7: KeyCode = 65;
    pub const KEY_F8: KeyCode = 66;
    pub const KEY_F9: KeyCode = 67;
    pub const KEY_F10: KeyCode = 68;
    pub const KEY_NUMLOCK: KeyCode = 69;
    pub const KEY_SCROLLLOCK: KeyCode = 70;
    pub const KEY_KP7: KeyCode = 71;
    pub const KEY_KP8: KeyCode = 72;
    pub const KEY_KP9: KeyCode = 73;
    pub const KEY_KPMINUS: KeyCode = 74;
    pub const KEY_KP4: KeyCode = 75;
    pub const KEY_KP5: KeyCode = 76;
    pub const KEY_KP6: KeyCode = 77;
    pub const KEY_KPPLUS: KeyCode = 78;
    pub const KEY_KP1: KeyCode = 79;
    pub const KEY_KP2: KeyCode = 80;
    pub const KEY_KP3: KeyCode = 81;
    pub const KEY_KP0: KeyCode = 82;
    pub const KEY_KPDOT: KeyCode = 83;

    pub const KEY_F11: KeyCode = 87;
    pub const KEY_F12: KeyCode = 88;

    pub const KEYCODE_MAX: KeyCode = 88;
}
