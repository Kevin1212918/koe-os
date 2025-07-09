use core::arch::asm;
use core::ops::Range;

use bitvec::field::BitField as _;
use bitvec::order::Lsb0;
use bitvec::view::BitView as _;

use crate::common::Privilege;

pub fn init() {
    static mut GDT: Gdt = Gdt([const { SegmentDesc::invalid() }; Gdt::LEN]);
    // SAFETY: GDT is not accessed outside of this function.
    unsafe { GDT.0[1] = SegmentDesc::code() };

    let gdtr = Gdtr {
        limit: (Gdt::LEN * size_of::<SegmentDesc>() - 1) as u16,
        // SAFETY: place expr is safe
        base: unsafe { &raw const GDT },
    };

    // SAFETY: loading a valid gdt is safe.
    unsafe {
        asm!(
            "lgdt [{gdtr}]",
            gdtr = in(reg) &gdtr as *const Gdtr
        )
    };
}

#[repr(C, packed(2))]
struct Gdtr {
    limit: u16,
    base: *const Gdt,
}


#[repr(C, align(8))]
struct Gdt([SegmentDesc; Self::LEN]);
impl Gdt {
    const LEN: usize = 2;
}
#[repr(C, packed)]
struct SegmentDesc(u64);
impl SegmentDesc {
    const DEFAULT_IDXS: Range<usize> = 54..55;
    const DESC_TYPE_IDXS: Range<usize> = 44..45;
    const DPL_IDXS: Range<usize> = 45..47;
    const GRANULARITY_IDXS: Range<usize> = 55..56;
    const LONG_MODE_IDXS: Range<usize> = 53..54;
    const P_IDXS: Range<usize> = 47..48;
    const TYPE_IDXS: Range<usize> = 40..44;

    fn code() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::TYPE_IDXS].store_le(0b1000);
        view[Self::DESC_TYPE_IDXS].store_le(1);
        view[Self::DPL_IDXS].store_le(Privilege::Kernel as u8);
        view[Self::P_IDXS].store_le(1);
        view[Self::LONG_MODE_IDXS].store(1);
        Self(bits)
    }

    const fn invalid() -> Self { Self(0) }
}
