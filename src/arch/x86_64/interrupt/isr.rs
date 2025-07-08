use core::arch::{asm, global_asm};
use core::cell::SyncUnsafeCell;
use core::ops::Range;

use bitvec::field::BitField as _;
use bitvec::order::Lsb0;
use bitvec::view::BitView as _;
use spin::Mutex;

use crate::common::Privilege;

global_asm!(include_str!("isr.S"));
unsafe extern "C" {
    pub static ISR_TABLE: [u64; 256];
}


pub fn init_idtr() {
    let idtr = Idtr {
        limit: (Idt::LEN * size_of::<InterruptDesc>()) as u16,
        base: IDT.get(),
    };
    // SAFETY: loading valid interrupt descriptor table is safe.
    unsafe {
        asm!(
            "lidt [{idtr}]",
            idtr = in(reg) &idtr as *const Idtr
        )
    };
}

pub fn init_exn_handlers() {
    let mut idt = IDT_HANDLE.lock();

    for i in 0..=21 {
        // SAFETY: Taking access to interrupt service routine table.
        let addr = unsafe { ISR_TABLE[i] };
        if addr == 0 {
            continue;
        }
        idt.0[i] = InterruptDesc::exn(addr);
    }
}

pub fn init_irq_handlers() {
    let mut idt = IDT_HANDLE.lock();

    for i in 32..=47 {
        // SAFETY: Taking address of interrupt service routine table.
        let addr = unsafe { ISR_TABLE[i] };
        if addr == 0 {
            continue;
        }
        idt.0[i] = InterruptDesc::irq(addr);
    }
}

#[repr(C, packed(2))]
struct Idtr {
    limit: u16,
    base: *mut Idt,
}

static IDT: SyncUnsafeCell<Idt> = SyncUnsafeCell::new(Idt([InterruptDesc::null(); Idt::LEN]));
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
    const DPL_IDXS: Range<usize> = 13..15;
    const IST_IDXS: Range<usize> = 0..2;
    const P_IDXS: Range<usize> = 15..16;
    const TYPE_IDXS: Range<usize> = 8..12;

    fn exn(addr: u64) -> Self { Self::new(addr, GateTyp::Trap, Privilege::Kernel) }
    fn irq(addr: u64) -> Self { Self::new(addr, GateTyp::Intrpt, Privilege::Kernel) }
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
    const fn null() -> Self {
        Self {
            low_low_offset: 0,
            segment_selector: 0,
            attributes: 0,
            high_low_offset: 0,
            high_offset: 0,
            _reserved: 0,
        }
    }
}
impl Default for InterruptDesc {
    fn default() -> Self { Self::null() }
}
pub type IntrptVector = u8;

pub(super) const VECTOR_DE: IntrptVector = 0;
pub(super) const VECTOR_DB: IntrptVector = 1;
pub(super) const VECTOR_NMI: IntrptVector = 2;
pub(super) const VECTOR_BP: IntrptVector = 3;
pub(super) const VECTOR_OF: IntrptVector = 4;
pub(super) const VECTOR_BR: IntrptVector = 5;
pub(super) const VECTOR_UD: IntrptVector = 6;
pub(super) const VECTOR_NM: IntrptVector = 7;
pub(super) const VECTOR_DF: IntrptVector = 8;
pub(super) const VECTOR_OMF: IntrptVector = 9;
pub(super) const VECTOR_TS: IntrptVector = 10;
pub(super) const VECTOR_NP: IntrptVector = 11;
pub(super) const VECTOR_SS: IntrptVector = 12;
pub(super) const VECTOR_GP: IntrptVector = 13;
pub(super) const VECTOR_PF: IntrptVector = 14;
pub(super) const VECTOR_MF: IntrptVector = 16;
pub(super) const VECTOR_AC: IntrptVector = 17;
pub(super) const VECTOR_MC: IntrptVector = 18;
pub(super) const VECTOR_XF: IntrptVector = 19;
pub(super) const VECTOR_VE: IntrptVector = 20;
pub(super) const VECTOR_CP: IntrptVector = 21;

pub(super) const VECTOR_PIC: IntrptVector = 32;
pub(super) const VECTOR_TIMER: IntrptVector = 32;
pub(super) const VECTOR_KEYBOARD: IntrptVector = 32;

#[repr(C)]
#[derive(Debug)]
pub struct IntrptStack {
    pub errno: usize,
    pub ip: usize,
    pub cs: usize,
    pub flags: usize,
    pub sp: usize,
    pub ss: usize,
}
