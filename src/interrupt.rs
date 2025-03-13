use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::ops::Range;
use core::{array, ptr};

use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use handler::default_handler;
use spin::Mutex;

use crate::common::{hlt, Privilege};

mod handler;

pub fn init() {
    init_idtr();
    let mut idt = IDT_HANDLE.lock();
    for i in 0..22 {
        let handler = default_handler as u64;
        idt.0[i] = InterruptDesc::new(
            handler as u64,
            GateTyp::Intrpt,
            Privilege::Kernel,
        );
    }

    enable_interrupt();
}

// x86-64 stuff
fn enable_interrupt() {
    unsafe {
        asm!("sti");
    }
}

fn init_idtr() {
    let idtr = Idtr {
        limit: (Idt::LEN * size_of::<InterruptDesc>()) as u16,
        base: IDT.get(),
    };

    unsafe {
        asm!(
            "lidt [{idtr}]",
            idtr = in(reg) &idtr as *const Idtr
        )
    };
}

#[repr(C, packed(2))]
struct Idtr {
    limit: u16,
    base: *mut Idt,
}

static IDT: SyncUnsafeCell<Idt> = SyncUnsafeCell::new(Idt([InterruptDesc::ABSENT; Idt::LEN]));
static IDT_HANDLE: Mutex<&'static mut Idt> =
    spin::Mutex::new(unsafe { IDT.get().as_mut_unchecked() });

#[repr(C, align(16))]
#[derive(Debug)]
struct Idt([InterruptDesc; Self::LEN]);
impl Idt {
    const LEN: usize = 256;
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct InterruptDesc {
    low_low_offset: u16,
    segment_selector: u16,
    attributes: u16,
    high_low_offset: u16,
    high_offset: u32,
    _reserved: u32,
}

#[repr(u8)]
enum GateTyp {
    Intrpt = 0b1110,
    Trap = 0b1111,
}
impl InterruptDesc {
    const ABSENT: Self = Self {
        low_low_offset: 0,
        segment_selector: 0,
        attributes: 0,
        high_low_offset: 0,
        high_offset: 0,
        _reserved: 0,
    };
    const DPL_IDXS: Range<usize> = 13..15;
    const IST_IDXS: Range<usize> = 0..2;
    const P_IDXS: Range<usize> = 15..16;
    const TYPE_IDXS: Range<usize> = 8..12;

    fn new(addr: u64, typ: GateTyp, dpl: Privilege) -> Self {
        let addr_bits = addr.view_bits::<Lsb0>();
        let low_low_offset = addr_bits[0..16].load_le();
        let high_low_offset = addr_bits[16..32].load_le();
        let high_offset = addr_bits[32..64].load_le();

        // NOTE: The CS segment selector should be 8.
        let segment_selector = 8;
        let _reserved = 0;

        // TODO: Implement interrupt stack table
        let mut attributes = 0;
        let attributes_bits = attributes.view_bits_mut::<Lsb0>();
        attributes_bits[Self::TYPE_IDXS].store_le(typ as u8);
        attributes_bits[Self::DPL_IDXS].store_le(dpl as u8);
        attributes_bits[Self::P_IDXS].store_le(1);

        Self {
            low_low_offset,
            segment_selector,
            attributes,
            high_low_offset,
            high_offset,
            _reserved,
        }
    }
}
impl Default for InterruptDesc {
    fn default() -> Self { Self::ABSENT }
}
