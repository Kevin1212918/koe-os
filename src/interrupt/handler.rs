use alloc::alloc::Global;
use alloc::boxed::Box;
use core::arch::{asm, global_asm};
use core::mem::offset_of;

use super::pic::ack;
use super::{
    timer, InterruptGuard, InterruptStack, InterruptVector, VECTOR_DF, VECTOR_PF, VECTOR_PIC,
};
use crate::common::ll::boxed::BoxLinkedListExt;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::common::log::error;
use crate::common::{die, log};
use crate::drivers::ps2;
use crate::interrupt::timer::timer_handler;
use crate::interrupt::VECTOR_TIMER;


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

const _: () = assert!(size_of::<InterruptGuard>() == 0);
#[no_mangle]
extern "C" fn exception_handler(vec: InterruptVector, stack: &InterruptStack) {
    match vec {
        VECTOR_PF => page_fault_handler(stack),
        VECTOR_DF => double_fault_handler(stack),
        _ => default_exn_handler(),
    }
}

// TODO: Change to atomic linked list
pub static IRQ_TABLE: [spin::Mutex<Option<LinkedList<IRQ_ITEM_LINK_OFFSET, Box<IrqItem>>>>; 16] =
    [const { spin::Mutex::new(None) }; 16];

const IRQ_ITEM_LINK_OFFSET: usize = offset_of!(IrqItem, link);
unsafe impl Linked<IRQ_ITEM_LINK_OFFSET> for IrqItem {}
struct IrqItem {
    f: IrqRoutine,
    link: Link,
}

/// Top-half irq handling routine.
///
/// This executes in an interrupt disabled context by the kernel irq handler.
pub type IrqRoutine = fn(&InterruptStack, &InterruptGuard);

/// Registers an irq routine to be called during an interrupt from the interrupt
/// vector `vec`.
///
/// # Undefined Behavior
/// - `vec` should be a valid irq vector.
/// - `vec` should not be the timer vector, which is managed by kernel.
fn register_irq_routine(vec: InterruptVector, f: IrqRoutine) {
    debug_assert!((33..=47).contains(&vec));
    if (33..=47).contains(&vec) {
        return;
    }
    let vec = vec - VECTOR_PIC;
    let item = Box::new(IrqItem {
        f,
        link: Link::new(),
    });

    let _intrpt = InterruptGuard::new();
    let mut irq_item_list = IRQ_TABLE[vec as usize].lock();
    let irq_item_list = irq_item_list
        .get_or_insert_with(|| LinkedList::<IRQ_ITEM_LINK_OFFSET, Box<IrqItem>>::new_in(Global));
    irq_item_list.push_back(item);
}

fn default_irq_handler(irq: u8, stack: &InterruptStack, intrpt: InterruptGuard) {
    let irq_item_list = IRQ_TABLE[irq as usize].lock();
    let Some(irq_item_list) = irq_item_list.as_ref() else {
        ack(irq);
        return;
    };
    for item in irq_item_list.iter() {
        (item.f)(stack, &intrpt);
    }
    ack(irq);
}

#[no_mangle]
extern "C" fn irq_handler(vec: InterruptVector, stack: &InterruptStack) {
    // NOTE: Exception handler enters with interrupt disabled, but the count on
    // interrupt guard is incorrect. We need to manually create the interrupt
    // guard to correct the count.
    let intrpt = InterruptGuard::new();
    match vec {
        VECTOR_TIMER => timer_handler(intrpt),
        _ => default_irq_handler(vec - VECTOR_PIC, stack, intrpt),
    }
}

// x86-64 stuff
global_asm!(include_str!("handler.S"));
unsafe extern "C" {
    pub static ISR_TABLE: [u64; 256];
}
