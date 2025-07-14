use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::ops::Range;
use core::sync::atomic::Ordering;

use atomic::Atomic;
use bitvec::field::BitField as _;
use bitvec::order::Lsb0;
use bitvec::view::BitView as _;

use crate::common::{Privilege, StackPtr};
use crate::interrupt::IntrptGuard;

pub fn init() {
    static mut GDT: Gdt = Gdt([const { SegmentDesc::invalid() }; Gdt::LEN]);
    // SAFETY: GDT is not accessed outside of this function.
    unsafe {
        GDT.0[1] = SegmentDesc::kcode();
        GDT.0[2] = SegmentDesc::kdata();
        GDT.0[3] = SegmentDesc::ucode();
        GDT.0[4] = SegmentDesc::udata();
        let [tss_low, tss_hi] = SegmentDesc::tss();
        GDT.0[5] = tss_low;
        GDT.0[6] = tss_hi;
    }

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

#[no_mangle]
static KERNEL_ENTRY_STACK_PTR: Atomic<StackPtr> = Atomic::new(0);
static TSS: SyncUnsafeCell<Tss> = const {
    let mut arr = [0; 26];
    arr[0] = size_of::<Tss>() as u32;
    SyncUnsafeCell::new(Tss(arr))
};
#[repr(C)]
struct Tss([u32; 26]);
impl Tss {
    pub fn set_kernel_entry_stack(&mut self, sp: StackPtr, intrpt: &IntrptGuard) {
        // Truncating cast
        let sp_low = sp as u32;
        let sp_hi = (sp >> 32) as u32;

        self.0[1] = sp_low;
        self.0[2] = sp_hi;
    }
}

/// Sets the user-to-kernel interrupt stack.
///
/// Interrupt service routine will adopt this stack when trapping from user to
/// kernel.
///
/// # Safety
/// - Caller should synchronize the calls externally.
/// - `sp` should point to a valid stack to be used for ISR.
pub unsafe fn set_kernel_entry_stack(sp: StackPtr, intrpt: Option<&IntrptGuard>) {
    let mut guard = None;
    let guard_ref = intrpt.unwrap_or_else(|| {
        guard = Some(IntrptGuard::new());
        // SAFETY: guard is initialized above.
        unsafe { guard.as_ref().unwrap_unchecked() }
    });
    // SAFETY: Caller synchronizes the call.
    unsafe { TSS.get().as_mut_unchecked() }.set_kernel_entry_stack(sp, guard_ref);
    KERNEL_ENTRY_STACK_PTR.store(sp, Ordering::Relaxed);
}

#[repr(C, packed(2))]
struct Gdtr {
    limit: u16,
    base: *const Gdt,
}
#[repr(C, align(8))]
struct Gdt([SegmentDesc; Self::LEN]);
impl Gdt {
    const LEN: usize = 7;
}
#[repr(C, packed)]
struct SegmentDesc(u64);
impl SegmentDesc {
    const LIMIT_LO: Range<usize> = 0..16;
    const BASE_LO: Range<usize> = 16..40;
    const ACCESS: Range<usize> = 40..48;
    const LIMIT_HI: Range<usize> = 48..52;
    const FLAGS: Range<usize> = 52..56;
    const BASE_HI: Range<usize> = 56..64;

    fn kcode() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::ACCESS].store_le(0x9A);
        view[Self::FLAGS].store_le(0xA);
        Self(bits)
    }

    fn kdata() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::ACCESS].store_le(0x92);
        view[Self::FLAGS].store_le(0xC);
        Self(bits)
    }

    fn ucode() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::ACCESS].store_le(0xFA);
        view[Self::FLAGS].store_le(0xA);
        Self(bits)
    }

    fn udata() -> Self {
        let mut bits = 0u64;
        let view = bits.view_bits_mut::<Lsb0>();
        view[Self::ACCESS].store_le(0xF2);
        view[Self::FLAGS].store_le(0xC);
        Self(bits)
    }

    fn tss() -> [Self; 2] {
        let mut tss_base: u64 = TSS.get() as u64;
        let tss_limit: u16 = size_of::<Tss>() as u16;
        let mut low = 0u64;
        let view = low.view_bits_mut::<Lsb0>();
        view[Self::ACCESS].store_le(0x89);
        view[Self::FLAGS].store_le(0x0);

        view[Self::LIMIT_LO].store_le(tss_limit);
        // We are not using LIMIT_HI

        view[Self::BASE_LO].store_le(tss_base);
        tss_base >>= Self::BASE_LO.len();
        view[Self::BASE_HI].store_le(tss_base);
        tss_base >>= Self::BASE_HI.len();

        let low = Self(low);
        let hi = Self(tss_base);

        [low, hi]
    }

    const fn invalid() -> Self { Self(0) }
}
