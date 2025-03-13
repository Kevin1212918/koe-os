use core::array;
use core::cell::SyncUnsafeCell;
use core::ops::Range;

use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;

use crate::common::Privilege;

// x86-64 stuff
static IDT: SyncUnsafeCell<Idt> = SyncUnsafeCell::new(Idt([InterruptDesc::ABSENT; 256]));

#[repr(C, align(16))]
#[derive(Debug)]
struct Idt([InterruptDesc; 256]);

#[repr(C, packed)]
#[derive(Debug, Clone)]
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

    fn new(offset: u64, typ: GateTyp, dpl: Privilege) -> Self {
        let offset_bits = offset.view_bits::<Lsb0>();
        let low_low_offset = offset_bits[0..16].load_le();
        let high_low_offset = offset_bits[16..32].load_le();
        let high_offset = offset_bits[32..64].load_le();

        // NOTE: The CS segment selector should be 0.
        let segment_selector = 0;
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
