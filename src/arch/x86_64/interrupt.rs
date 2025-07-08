use core::arch::asm;

mod handler;
mod isr;
mod pic;
mod timer;

pub fn init() {
    isr::init_idtr();
    isr::init_exn_handlers();
    isr::init_irq_handlers();
    pic::init_pic();

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

pub use handler::register_handler;
