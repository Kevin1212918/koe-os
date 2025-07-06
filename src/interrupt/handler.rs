use core::arch::{asm, global_asm};

use super::pic::ack;
use super::{timer, InterruptStack, InterruptVector, VECTOR_DF, VECTOR_PF, VECTOR_PIC};
use crate::common::log::error;
use crate::common::{die, log};
use crate::drivers::ps2;


#[repr(transparent)]
#[derive(Clone, Copy)]
struct Isr(pub extern "C" fn());

fn page_fault_handler(stack: &InterruptStack) {
    let cr2: u64;
    unsafe {
        asm! {"mov r11, cr2", out("r11") cr2}
    };
    error!("Page Fault!");
    error!("cr2: {:#x}", cr2);
    error!("{:?}", stack);
    die();
}

fn double_fault_handler(stack: &InterruptStack) {
    error!("Double Fault!");
    die();
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
        0 => timer::timer_handler(),
        1 => ps2::ps2_keyboard_handler(),
        _ => (),
    }
    ack(irq);

    match irq {
        0 => timer::timer_scheduler(),
        _ => (),
    }
}
// x86-64 stuff
global_asm!(include_str!("handler.S"));
unsafe extern "C" {
    pub static ISR_TABLE: [u64; 256];
}
