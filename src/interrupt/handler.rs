use core::arch::global_asm;

use super::pic::ack;
use super::{InterruptStack, InterruptVector, VECTOR_DF, VECTOR_PF, VECTOR_PIC};
use crate::common::log::error;
use crate::common::{hlt, log};
use crate::drivers::ps2;


#[repr(transparent)]
#[derive(Clone, Copy)]
struct Isr(pub extern "C" fn());

fn page_fault_handler(stack: &InterruptStack) {
    error!("Page Fault!");
    hlt();
}

fn double_fault_handler(stack: &InterruptStack) {
    error!("Double Fault!");
    hlt();
}

fn default_exn_handler() {}

#[no_mangle]
pub extern "C" fn exception_handler(vec: InterruptVector, stack: &InterruptStack) {
    match vec {
        VECTOR_PF => page_fault_handler(stack),
        VECTOR_DF => double_fault_handler(stack),
        _ => default_exn_handler(),
    }
}

#[no_mangle]
pub extern "C" fn irq_handler(vec: InterruptVector, stack: &InterruptStack) {
    let irq = vec - VECTOR_PIC;
    match irq {
        1 => ps2::ps2_keyboard_handler(),
        _ => (),
    }
    ack(irq);
}
// x86-64 stuff
global_asm!(include_str!("handler.S"));
unsafe extern "C" {
    pub static ISR_TABLE: [u64; 256];
}
