use core::arch::global_asm;

use crate::common::{InstrPtr, StackPtr};

global_asm!(include_str!("enter_usr.S"));
unsafe extern "C" {
    pub fn enter_usr(sp: StackPtr, ip: InstrPtr) -> !;
}
