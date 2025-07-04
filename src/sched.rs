use alloc::alloc::Global;
use alloc::boxed::Box;
use core::arch::global_asm;
use core::cell::{SyncUnsafeCell, UnsafeCell};
use core::hint::unreachable_unchecked;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};

use switch::switch_to;
use thread::{Thread, THREAD_LINK_OFFSET};

use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::interrupt::InterruptGuard;
use crate::mem::addr::PageSize;
use crate::mem::{PageAllocator, PhysicalRemapSpace, UserSpace};

mod switch;
pub mod thread;

static SCHED: spin::Lazy<Scheduler> = spin::Lazy::new(|| Scheduler::new());

/// Per-CPU structure managing currently running threads.
struct SchedulerRecord {
    running_thread: Option<Box<Thread>>,
    ready_threads: LinkedList<THREAD_LINK_OFFSET, Box<Thread>>,
    zombie_threads: LinkedList<THREAD_LINK_OFFSET, Box<Thread>>,
}
struct Scheduler(spin::Mutex<SchedulerRecord>);
impl Scheduler {
    fn new() -> Self {
        Scheduler(spin::Mutex::new(SchedulerRecord {
            running_thread: None,
            ready_threads: LinkedList::<THREAD_LINK_OFFSET, Box<Thread>>::new_in(Global),
            zombie_threads: LinkedList::<THREAD_LINK_OFFSET, Box<Thread>>::new_in(Global),
        }))
    }

    /// Creates and schedule a new kernel thread which will call `main`.
    pub fn new_thread(main: fn()) {}
    /// Schedules a new thread.
    ///
    /// # Safety
    /// Stack of the new thread should be safe to be switched to.
    unsafe fn schedule(new: Box<Thread>) {}

    /// Yield to another thread if available.
    pub fn yield_thread(&self) { Self::reschedule(ThreadState::Ready); }

    fn reschedule(new_state: ThreadState) {
        let cur_new_opt = Self::before_switch(new_state);
        let Some((cur, new)) = cur_new_opt else {
            return;
        };
        // SAFETY: cur and new are valid as guarenteed by before_switch. Interrupt and
        // scheduler are locked by before_switch.
        unsafe { switch_to(cur, new) };
        // SAFETY: called incorrespondance to before_switch.
        unsafe { Self::after_switch() };
    }

    /// Prepares thread switch.
    ///
    /// Disable interrupt and lock the scheduler.
    ///
    /// Move current thread from `running_thread` to the thread queue as
    /// specified by `ThreadState`, then try find a new thread from
    /// `ready_threads` into the `running_thread` to be executed.
    ///
    /// Returns `None` if no new thread is available to execute. Otherwise
    /// return a pointer to current thread's `rsp` field, and the new
    /// thread's `rsp`.
    fn before_switch(new_state: ThreadState) -> Option<(*mut u64, u64)> {
        // Disable interrupt on current core.
        InterruptGuard::raw_lock();
        // Lock scheduler for the current core.
        let sched = spin::MutexGuard::leak(SCHED.0.lock());

        // SAFETY: The access does not overlap with other access since we are holding
        // scheduler lock.
        let new_thread = sched.ready_threads.front_mut().remove()?;
        let new_thread_rsp = new_thread.meta.rsp;
        let mut cur_thread = sched
            .running_thread
            .replace(new_thread)
            .expect("There should always be a running thread");

        let que = match new_state {
            ThreadState::Ready => &mut sched.ready_threads,
            ThreadState::Zombie => &mut sched.zombie_threads,
            // SAFETY: we check at start of the function that new_state is not running.
            ThreadState::Running => unsafe { unreachable_unchecked() },
        };

        let cur_thread_rsp = &raw mut cur_thread.meta.rsp;
        que.push_back(cur_thread);
        Some((cur_thread_rsp, new_thread_rsp))
    }

    /// Clean up after thread switch.
    ///
    /// Enables interrupt and unlock the scheduler.
    ///
    /// # Safety
    /// This should only be called in correspondance to a previously called
    /// `before_switch` on the current **CPU**.
    unsafe fn after_switch() {
        // SAFEtY: Unlock scheduler for the current core.
        unsafe { SCHED.0.force_unlock() };
        // SAFETY: Enable interrupt on the current core.
        unsafe { InterruptGuard::raw_unlock() };
    }
}

enum ThreadState {
    Running,
    Ready,
    Zombie,
}
