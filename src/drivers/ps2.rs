use core::cell::SyncUnsafeCell;

use arraydeque::ArrayDeque;
use arrayvec::ArrayVec;

use super::input::keycode::*;
use super::input::KEYBOARD_Q;
use crate::common::pmio::{inb, Port, RPort, WPort};
use crate::interrupt::InterruptGuard;

const DATA_PORT: Port = Port(0x60);
const STATUS_PORT: RPort = RPort(0x64);
const CMD_PORT: WPort = WPort(0x64);

static SCANCODE: SyncUnsafeCell<Sc1> = SyncUnsafeCell::new(Sc1::Normal);

/// FIXME: UB on multiprocessor
fn ps2_keyboard_handler() {
    let sc = unsafe { SCANCODE.get().as_mut_unchecked() };
    let byte = inb(DATA_PORT);
    let Some((key, is_pressed)) = sc.parse(byte) else {
        return;
    };
    if is_pressed {
        KEYBOARD_Q.lock().push_back(key);
    }
}

enum Sc1 {
    Normal,
    Extra(u8),
    Pause(u8),
    Command,
}
impl Sc1 {
    fn parse(&mut self, byte: u8) -> Option<(KeyCode, bool)> {
        fn parse_normal(sc1: &mut Sc1, byte: u8) -> Option<(KeyCode, bool)> {
            let byte = byte as u16;

            const KEY_RESERVED_R: KeyCode = KEY_RESERVED + 0x80;
            const KEY_KPDOT_R: KeyCode = KEY_KPDOT + 0x80;
            const KEY_F11_R: KeyCode = KEY_F11 + 0x80;
            const KEY_F12_R: KeyCode = KEY_F12 + 0x80;

            match byte {
                KEY_RESERVED..=KEY_KPDOT | KEY_F11..=KEY_F12 => Some((byte as KeyCode, true)),
                KEY_RESERVED_R..=KEY_KPDOT_R | KEY_F11_R..=KEY_F12_R =>
                    Some((byte as KeyCode, false)),
                0xE0 => {
                    // *sc1 = Sc1::Extra(0xE0);
                    None
                },
                0xE1 => {
                    // *sc1 = Sc1::Pause(0xE1);
                    None
                },
                _ => None, // Not parsed
            }
        }

        match self {
            Sc1::Normal => parse_normal(self, byte),
            Sc1::Extra(_) => todo!(),
            Sc1::Pause(_) => todo!(),
            Sc1::Command => todo!(),
        }
    }
}
