use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use crate::common::log::ok;
use crate::sched::{self, Scheduler};

pub fn schedule_timer_task() {}

static TICK: AtomicU32 = AtomicU32::new(0);

pub fn timer_handler() { TICK.fetch_add(1, Ordering::Relaxed); }
// TODO: This only works with kernel stacks.
pub fn timer_scheduler() {
    if TICK.load(Ordering::Relaxed) % 16 == 0 {
        sched::yield_kthread();
    }
}
