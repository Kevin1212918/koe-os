use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;

use arraydeque::ArrayDeque;
use arrayvec::ArrayVec;
use ringbuf::traits::{Consumer, Producer, Split, SplitRef};
use ringbuf::HeapRb as Rb;

use crate::common::pmio::{inb, Port, RPort, WPort};
use crate::drivers::vga::VGA_BUFFER;
use crate::interrupt::InterruptGuard;
use crate::io::keyboard::keycode::*;
use crate::io::keyboard::{KeyEvent, Keyboard, VirtKeyboard};
use crate::log;

const DATA_PORT: Port = Port(0x60);
const STATUS_PORT: RPort = RPort(0x64);
const CMD_PORT: WPort = WPort(0x64);

static KEYBOARD_SRC: spin::Once<SyncUnsafeCell<Ps2KeyboardSrc>> = spin::Once::new();
pub static KEYBOARD: spin::Once<SyncUnsafeCell<Ps2Keyboard>> = spin::Once::new();

pub fn init() {
    let key_buffer = Rb::new(128);
    let (prod, cons) = key_buffer.split();
    KEYBOARD_SRC.call_once(|| {
        SyncUnsafeCell::new(Ps2KeyboardSrc {
            cur_sc: SyncUnsafeCell::new(Sc::Sc1(Sc1::Normal)),
            prod,
        })
    });
    KEYBOARD.call_once(|| {
        SyncUnsafeCell::new(Ps2Keyboard {
            virt: VirtKeyboard::new(),
            src: cons,
        })
    });
}

/// FIXME: UB on multiprocessor
pub fn ps2_keyboard_handler() {
    let byte = inb(DATA_PORT);
    let Some(src) = KEYBOARD_SRC.get() else {
        return;
    };
    let src = unsafe { src.get().as_mut_unchecked() };
    let sc = unsafe { src.cur_sc.get().as_mut_unchecked() };
    let Some(packet) = sc.parse(byte) else {
        return;
    };
    if !packet.1 {
        foo();
    }
    src.prod.try_push(packet);
}
fn foo() {}

pub struct Ps2Keyboard {
    virt: VirtKeyboard,
    src: <Rb<(KeyCode, bool)> as Split>::Cons,
}

// FIXME: Temporary workaround, not safe!
unsafe impl Sync for Ps2Keyboard {}
impl Keyboard for Ps2Keyboard {}
impl Iterator for Ps2Keyboard {
    type Item = KeyEvent;

    fn next(&mut self) -> Option<Self::Item> {
        let packet = self.src.try_pop()?;
        self.virt.parse(packet)
    }
}

struct Ps2KeyboardSrc {
    cur_sc: SyncUnsafeCell<Sc>,
    prod: <Rb<(KeyCode, bool)> as Split>::Prod,
}
// FIXME: Temporary workaround, not safe!
unsafe impl Sync for Ps2KeyboardSrc {}

enum Sc {
    Sc1(Sc1),
}
impl Sc {
    fn parse(&mut self, byte: u8) -> Option<(KeyCode, bool)> {
        match self {
            Self::Sc1(sc1) => sc1.parse(byte),
        }
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
                    Some((byte - 0x80 as KeyCode, false)),
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
