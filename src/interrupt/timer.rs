use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use super::pic::ack;
use super::InterruptGuard;
use crate::common::log::ok;
use crate::sched::{self, Scheduler, ThreadState};

static TICK: AtomicU32 = AtomicU32::new(0);

pub fn timer_handler(intrpt: InterruptGuard) {
    let tick = TICK.fetch_add(1, Ordering::Relaxed);
    ack(0);



    if tick % 16 == 0 {
        // TODO: This only works with kernel stacks.
        //
        // we pass the interrupt guard from irq handler to ensure no new
        // lock is created when rescheduling.
        //
        // intrpt will be dropped after this returns.
        sched::reschedule(ThreadState::Ready, intrpt);
    }
}
