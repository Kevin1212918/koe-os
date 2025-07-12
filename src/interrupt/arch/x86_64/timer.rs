use super::pic::ack;
use crate::interrupt::IntrptGuard;
use crate::sched::{self, ThreadState};

const PREEMPT_TICK: u64 = 16;
static mut TICK: u64 = 0;

pub fn tick() -> u64 { unsafe { TICK } }
pub fn timer_handler(intrpt: IntrptGuard) {
    let tick;
    // SAFETY: There is only one instance of timer_handler running.
    unsafe {
        tick = TICK;
        TICK += 1;
    }
    ack(0);

    if tick % PREEMPT_TICK == 0 {
        // TODO: This only works with kernel stacks.
        //
        // we pass the interrupt guard from irq handler to ensure no new
        // lock is created when rescheduling.
        //
        // intrpt will be dropped after this returns.
        sched::preempt(intrpt);
    }
}
