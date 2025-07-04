use core::arch::global_asm;

global_asm!(include_str!("switch.S"));
unsafe extern "C" {
    /// Switches current thread to new thread specified by `new_rsp`, and then
    /// switches back when/if current thread is scheduled back. Note current
    /// thread may be moved and continue execute on a different CPU.
    ///
    /// This function blocks until the current thread is switched back.
    ///
    /// Before calling `switch_to`, caller should disable interrupt with
    /// `InterruptGuard::raw_lock` for its executing CPU, and lock scheduler
    /// with `spin::MutexGuard::leak` for the CPU. The new thread may release
    /// the locks on interrupt and scheduler.
    ///
    /// After exiting from `switch_to`, interrupt on the new CPU is guarenteed
    /// to be disabled through `InterruptGuard::raw_lock`, and scheduler for
    /// the CPU is locked using `spin::MutexGuard::leak`.
    ///
    /// # Safety
    /// - `old_rsp` should point to the currently executing `Thread`'s `rsp`
    ///   field.
    /// - `new_rsp` should point to top of new `Thread`'s stack.
    /// - Interrupt should be disabled through `InterruptGuard::raw_lock`.
    /// - Scheduler should be disabled through `spin::MutexGuard::leak`.
    pub unsafe fn switch_to(old_rsp: *mut u64, new_rsp: u64);
}
