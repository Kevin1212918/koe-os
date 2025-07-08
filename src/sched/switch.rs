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
    /// `IntrptGuard::raw_lock` for its executing CPU. The new thread may
    /// release the locks on interrupt.
    ///
    /// After exiting from `switch_to`, interrupt on the new CPU is guarenteed
    /// to be disabled through `IntrptGuard::raw_lock`.
    ///
    /// # Safety
    /// - `old_rsp` should point to the currently executing `Thread`'s `rsp`
    ///   field.
    /// - `new_rsp` should point to top of new `Thread`'s stack.
    /// - Interrupt should be disabled through `IntrptGuard::raw_lock`.
    pub unsafe fn switch_to(old_rsp: *mut usize, new_rsp: usize);
}
