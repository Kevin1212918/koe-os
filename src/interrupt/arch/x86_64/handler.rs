use alloc::alloc::Global;
use alloc::boxed::Box;
use core::arch::{asm, global_asm};
use core::mem::offset_of;
use core::ops::DerefMut;

use super::isr::{IntrptStack, IntrptVector, VECTOR_PIC};
use super::pic::ack;
use super::timer::timer_handler;
use crate::arch::die;
use crate::common::ll::boxed::BoxLinkedListExt;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::common::log::error;
use crate::drivers::ps2;
use crate::interrupt::irq::{Handler, IrqInfo, IrqVector};
use crate::interrupt::IntrptGuard;


#[repr(transparent)]
#[derive(Clone, Copy)]
struct Isr(pub extern "C" fn());

fn page_fault_handler(stack: &IntrptStack) {
    let cr2: u64;
    unsafe {
        asm! {"mov r11, cr2", out("r11") cr2}
    };
    error!("Page Fault!");
    error!("cr2: {:#x}", cr2);
    error!("{:?}", stack);
    die();
}

fn double_fault_handler(stack: &IntrptStack) {
    error!("Double Fault!");
    die();
}

fn default_exn_handler() {}

const _: () = assert!(size_of::<IntrptGuard>() == 0);
#[no_mangle]
extern "C" fn exception_sr(vec: IntrptVector, stack: &IntrptStack) {
    match vec {
        VECTOR_PF => page_fault_handler(stack),
        VECTOR_DF => double_fault_handler(stack),
        _ => default_exn_handler(),
    }
}



#[no_mangle]
extern "C" fn irq_sr(vec: IntrptVector, stack: &IntrptStack) {
    // NOTE: Exception handler enters with interrupt disabled, but the count on
    // interrupt guard is incorrect. We need to manually create the interrupt
    // guard to correct the count.
    let intrpt = IntrptGuard::new();
    match vec {
        VECTOR_TIMER => timer_handler(intrpt),
        _ => default_irq_handler(from_intrpt_vector(vec), stack, intrpt),
    }
}

const fn from_intrpt_vector(vec: IntrptVector) -> IrqVector { vec - VECTOR_PIC }

const fn build_irq_info(stack: &IntrptStack) -> IrqInfo {
    IrqInfo {
        errno: stack.errno,
        ip: stack.ip,
        sp: stack.sp,
    }
}

fn default_irq_handler(irq: IrqVector, stack: &IntrptStack, intrpt: IntrptGuard) {
    let irq_item_list = IRQ_HANDLER_TABLE[irq as usize].lock();
    let Some(irq_item_list) = irq_item_list.as_ref() else {
        ack(irq);
        return;
    };
    for item in irq_item_list.iter() {
        (item.f)(build_irq_info(stack), &intrpt);
    }
    ack(irq);
}

// TODO: Change to atomic linked list
pub static IRQ_HANDLER_TABLE: [spin::Mutex<Option<HandlerQ>>; 16] =
    [const { spin::Mutex::new(None) }; 16];

const ITEM_LINK_OFFSET: usize = offset_of!(Item, link);
unsafe impl Linked<ITEM_LINK_OFFSET> for Item {}
struct Item {
    f: Handler,
    link: Link,
}

type HandlerQ = LinkedList<ITEM_LINK_OFFSET, Box<Item>>;

/// Registers an irq routine to be called during an interrupt from the
/// interrupt vector `vec`.
///
/// # Undefined Behavior
/// - `vec` should be a valid irq vector.
/// - `vec` should not be the timer vector, which is managed by kernel.
pub fn register_handler(vec: IrqVector, f: Handler) {
    if !(33..=47).contains(&vec) {
        error!("register_handler should receive a valid irq vector.");
        return;
    }
    let vec = vec - VECTOR_PIC;
    let intrpt = IntrptGuard::new();

    let mut q = IRQ_HANDLER_TABLE[vec as usize].lock();
    let q = q.get_or_insert_with(|| LinkedList::<ITEM_LINK_OFFSET, Box<Item>>::new_in(Global));

    q.push_back(Box::new(Item {
        f,
        link: Link::new(),
    }));
}
