use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::ops::Range;
use core::sync::atomic::{self, AtomicUsize};

use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use handler::ISR_TABLE;
use pic::init_pic;
use spin::Mutex;

use crate::common::Privilege;

mod handler;
mod pic;

/// An RAII implementation of reentrant interrupt lock. This structure
/// guarentees that interrupt is disabled.
pub struct InterruptGuard();
impl InterruptGuard {
    pub fn new() -> Self {
        disable_interrupt();
        INTERRUPT_GUARD_CNT.fetch_add(1, atomic::Ordering::Relaxed);
        Self()
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        let prev_cnt = INTERRUPT_GUARD_CNT.fetch_sub(1, atomic::Ordering::Relaxed);
        if prev_cnt == 1 {
            enable_interrupt();
        }
    }
}
static INTERRUPT_GUARD_CNT: AtomicUsize = AtomicUsize::new(0);

pub type IrqHandler = fn();

// x86-64 stuff

pub fn init() {
    init_idtr();
    init_exn_handlers();
    init_irq_handlers();
    init_pic();

    pic::mask_all();
    pic::unmask(1);
    enable_interrupt();
}

fn enable_interrupt() {
    // SAFETY: enabling interrupt is safe.
    unsafe {
        asm!("sti");
    };
}

fn disable_interrupt() {
    // SAFETY: disabling interrupt is safe.
    unsafe {
        asm!("cli");
    };
}

fn init_idtr() {
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

fn init_exn_handlers() {
    let mut idt = IDT_HANDLE.lock();

    for i in 0..=21 {
        // SAFETY: Accessing interrupt service routine table.
        let addr = unsafe { ISR_TABLE[i] };
        if addr == 0 {
            continue;
        }
        idt.0[i] = InterruptDesc::exn(addr);
    }
}

fn init_irq_handlers() {
    let mut idt = IDT_HANDLE.lock();

    for i in 32..=47 {
        // SAFETY: Accessing interrupt service routine table.
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
    fn irq(addr: u64) -> Self { Self::new(addr, GateTyp::Trap, Privilege::Kernel) }
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
type InterruptVector = u8;

const VECTOR_DE: InterruptVector = 0;
const VECTOR_DB: InterruptVector = 1;
const VECTOR_NMI: InterruptVector = 2;
const VECTOR_BP: InterruptVector = 3;
const VECTOR_OF: InterruptVector = 4;
const VECTOR_BR: InterruptVector = 5;
const VECTOR_UD: InterruptVector = 6;
const VECTOR_NM: InterruptVector = 7;
const VECTOR_DF: InterruptVector = 8;
const VECTOR_OMF: InterruptVector = 9;
const VECTOR_TS: InterruptVector = 10;
const VECTOR_NP: InterruptVector = 11;
const VECTOR_SS: InterruptVector = 12;
const VECTOR_GP: InterruptVector = 13;
const VECTOR_PF: InterruptVector = 14;
const VECTOR_MF: InterruptVector = 16;
const VECTOR_AC: InterruptVector = 17;
const VECTOR_MC: InterruptVector = 18;
const VECTOR_XF: InterruptVector = 19;
const VECTOR_VE: InterruptVector = 20;
const VECTOR_CP: InterruptVector = 21;

const VECTOR_PIC: InterruptVector = 32;

#[repr(C)]
struct InterruptStack {
    errno: usize,
    ip: usize,
    cs: usize,
    flags: usize,
    sp: usize,
    ss: usize,
}
