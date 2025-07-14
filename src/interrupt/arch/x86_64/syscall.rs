use core::arch::global_asm;
use core::cell::SyncUnsafeCell;

use crate::common::StackPtr;

global_asm!(include_str!("syscall.S"));

pub fn init_syscall() {
    // SAFETY: Setting syscall msrs in initialization is safe
    unsafe { set_syscall_msrs() };
}

unsafe extern "C" {
    fn set_syscall_msrs();
}
