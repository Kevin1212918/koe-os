use core::arch::asm;

use crate::common::StackPtr;
use crate::interrupt::IntrptGuard;

mod handler;
mod isr;
mod pic;
mod syscall;
mod timer;

pub fn init() {
    isr::init_idtr();
    isr::init_exn_handlers();
    isr::init_irq_handlers();
    pic::init_pic();
    syscall::init_syscall();

    pic::mask_all();
    pic::unmask(0);
    pic::unmask(1);
    enable_interrupt();
}

pub fn enable_interrupt() {
    // SAFETY: enabling interrupt is safe.
    unsafe {
        asm!("sti");
    };
}

pub fn disable_interrupt() {
    // SAFETY: disabling interrupt is safe.
    unsafe {
        asm!("cli");
    };
}
