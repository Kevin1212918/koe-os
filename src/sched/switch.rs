use core::alloc::Layout;
use core::arch::global_asm;
use core::mem::MaybeUninit;
use core::ptr::slice_from_raw_parts;
use core::slice;

use super::kthread_entry;

global_asm!(include_str!("switch.S"));
unsafe extern "C" {
    /// Writes the current thread stack pointer to `old_rsp`, then switches
    /// current thread to new thread specified by `new_rsp`, and then
    /// switches back when/if current thread is scheduled back. Note current
    /// thread may be moved and continue execute on a different CPU.
    ///
    /// This function blocks until the current thread is switched back.
    ///
    /// Before calling `switch_to`, caller should disable interrupt with
    /// `InterruptGuard::raw_lock` for its executing CPU. The new thread may
    /// release the locks on interrupt.
    ///
    /// After exiting from `switch_to`, interrupt on the new CPU is guarenteed
    /// to be disabled through `InterruptGuard::raw_lock`.
    ///
    /// # Safety
    /// - `old_rsp` should point to the currently executing `Thread`'s `rsp`
    ///   field.
    /// - `new_rsp` should point to top of new `Thread`'s stack.
    /// - Interrupt should be disabled through `InterruptGuard::raw_lock`.
    pub unsafe fn switch_to(old_rsp: *mut usize, new_rsp: usize);
}

/// Initial `KThread` stack.
///
/// When building a new `KThread`, `INIT_KTHREAD_STACK` will be byte-wise copied
/// to the new thread's address aligned stack.
#[repr(C)]
pub(super) struct InitKThreadStack {
    null: u64, // This is here to fix kthread_entry alignment.
    kthread_entry: extern "C" fn() -> !,
    rbp: u64,
    rbx: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
}
impl InitKThreadStack {
    pub fn as_uninit_usizes(&self) -> &[MaybeUninit<usize>] {
        let base: *const MaybeUninit<usize> = (self as *const InitKThreadStack).cast();
        let len = size_of::<InitKThreadStack>() / size_of::<usize>();
        const _: () = assert!(size_of::<InitKThreadStack>() % size_of::<usize>() == 0);
        const _: () = assert!(align_of::<InitKThreadStack>() <= align_of::<usize>());

        // SAFETY:
        // base is not null since it comes from `self`.
        // base is aligned to usize since align of InitKThreadStack is smaller than that
        // of usize. the memory range pointed is len * size_of(usize), which
        // equals size_of(InitKThreadStack).
        // MaybeUninit is always initialized.
        // Lifetime inherited from `self` prevents mutation for duration of the
        // lifetime.
        // InitiKThreadStack size does not overflow.
        unsafe { slice::from_raw_parts(base, len) }
    }
}

pub(super) static INIT_KTHREAD_STACK: InitKThreadStack = InitKThreadStack {
    null: 0,
    kthread_entry,
    rbp: 0,
    rbx: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
};
