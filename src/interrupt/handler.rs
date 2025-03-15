use core::arch::global_asm;
use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;
use core::mem::MaybeUninit;
use core::ptr;

use super::{InterruptStack, InterruptVector, VECTOR_DF, VECTOR_PF};
use crate::common::hlt;
use crate::drivers::vga::VGA_BUFFER;
use crate::log;


#[repr(transparent)]
#[derive(Clone, Copy)]
struct Isr(pub extern "C" fn());

fn page_fault_handler(stack: &InterruptStack) {
    log!("Page Fault!");
    hlt();
}

fn double_fault_handler(stack: &InterruptStack) {
    log!("Double Fault!");
    hlt();
}

#[no_mangle]
pub extern "C" fn exception_handler(vec: InterruptVector, stack: &InterruptStack) {
    match vec {
        VECTOR_PF => page_fault_handler(stack),
        VECTOR_DF => double_fault_handler(stack),
        _ => (),
    }
}


// x86-64 stuff
global_asm!(include_str!("handler.S"));
unsafe extern "C" {
    pub unsafe static ISR_TABLE: [u64; 256];
}
