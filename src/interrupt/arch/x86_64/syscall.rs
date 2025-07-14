use core::arch::global_asm;
use core::cell::SyncUnsafeCell;

use crate::common::StackPtr;

global_asm!(include_str!("syscall.S"));

pub fn init_syscall() {}
