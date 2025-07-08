use core::arch::asm;
use core::cell::SyncUnsafeCell;
use core::mem::forget;
use core::ops::Range;
use core::sync::atomic::{self, AtomicUsize};

use bitvec::field::BitField;
use bitvec::order::Lsb0;
use bitvec::view::BitView;
use spin::Mutex;

use crate::arch;
use crate::arch::interrupt::{disable_interrupt, enable_interrupt};
use crate::common::{InstrPtr, Privilege, StackPtr};

pub fn init() { arch::interrupt::init(); }

/// An RAII implementation of reentrant interrupt lock. This structure
/// guarentees that interrupt is disabled.
pub struct IntrptGuard();
impl IntrptGuard {
    pub fn new() -> Self {
        disable_interrupt();
        INTERRUPT_GUARD_CNT.fetch_add(1, atomic::Ordering::Relaxed);
        Self()
    }
    /// # Safety
    /// `reclaim` should always correspond to a previously leaked guard.
    pub unsafe fn reclaim() -> Self { Self() }

    pub fn leak(self) { forget(self) }
    pub fn cnt() -> usize { INTERRUPT_GUARD_CNT.load(atomic::Ordering::Relaxed) }
}

impl Drop for IntrptGuard {
    fn drop(&mut self) {
        let prev_cnt = INTERRUPT_GUARD_CNT.fetch_sub(1, atomic::Ordering::Relaxed);
        if prev_cnt == 1 {
            enable_interrupt();
        }
    }
}

/// Per-CPU tracker for the number of interrupt guard in the kernel.
static INTERRUPT_GUARD_CNT: AtomicUsize = AtomicUsize::new(0);


pub mod irq {
    use super::IntrptGuard;
    pub use crate::arch::interrupt::register_handler;
    use crate::common::{InstrPtr, StackPtr};

    pub type IrqVector = u8;
    pub struct IrqInfo {
        pub errno: usize,
        pub ip: InstrPtr,
        pub sp: StackPtr,
    }

    /// Top-half irq handling routine.
    ///
    /// This executes in an interrupt disabled context by the kernel irq
    /// handler.
    pub type Handler = fn(IrqInfo, &IntrptGuard);
}
