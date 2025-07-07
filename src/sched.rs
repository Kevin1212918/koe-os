use alloc::alloc::Global;
use alloc::boxed::Box;
use core::arch::global_asm;
use core::cell::{SyncUnsafeCell, UnsafeCell};
use core::hint::unreachable_unchecked;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};
use core::pin::Pin;

use pinned_init::InPlaceInit;
use switch::switch_to;
use thread::{KThread, THREAD_LINK_OFFSET};

use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::common::log::{error, ok};
use crate::common::{die, hlt};
use crate::interrupt::InterruptGuard;
use crate::mem::addr::PageSize;
use crate::mem::{PageAllocator, PhysicalRemapSpace, UserSpace};

mod switch;
pub mod thread;

static SCHED: spin::Mutex<Option<Scheduler>> = spin::Mutex::new(None);

pub fn init_scheduler() {
    let mut sched_slot = SCHED.lock();
    let i1 = KThread::boxed(idle1, 1);
    let i2 = KThread::boxed(idle2, 1);
    let sched = sched_slot.insert(Scheduler::new());
    sched.schedule(i1, ThreadState::Ready);
    sched.schedule(i2, ThreadState::Ready);
}
/// # Safety
/// Should be called at the end of initialization to switch to the idle task.
pub fn init_switch_to_idle() -> ! {
    let idle = KThread::boxed(idle, u8::MAX);
    let new_rsp = idle.meta.rsp;
    Box::leak(idle);
    let mut dummy: usize = 0;
    InterruptGuard::new().leak();
    SCHED.lock().as_mut().unwrap().is_disabled = false;
    unsafe { switch_to(&raw mut dummy, new_rsp) };
    unreachable!()
}

/// Per-CPU structure managing currently running threads.
pub struct Scheduler {
    ready_threads: LinkedList<THREAD_LINK_OFFSET, Box<KThread>>,
    zombie_threads: LinkedList<THREAD_LINK_OFFSET, Box<KThread>>,
    idle: Option<Box<KThread>>,
    is_disabled: bool,
}
impl Scheduler {
    fn new() -> Self {
        Scheduler {
            ready_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KThread>>::new_in(Global),
            zombie_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KThread>>::new_in(Global),
            idle: None,
            is_disabled: true,
        }
    }

    fn next_kthread(&mut self) -> Option<Box<KThread>> {
        self.ready_threads
            .front_mut()
            .remove()
            .or_else(|| self.idle.take())
    }

    /// Return `None` if scheduler does not keep track of the state.
    fn queue_mut(
        &mut self,
        state: ThreadState,
    ) -> Option<&mut LinkedList<THREAD_LINK_OFFSET, Box<KThread>>> {
        match state {
            ThreadState::Ready => Some(&mut self.ready_threads),
            ThreadState::Zombie => Some(&mut self.zombie_threads),
            // SAFETY: we check at start of the function that new_state is not running.
            ThreadState::Running => None,
        }
    }

    /// UB when scheduling to running state.
    fn schedule(&mut self, thread: Box<KThread>, state: ThreadState) {
        debug_assert!(!matches!(state, ThreadState::Running));
        // Disable interrupt on current core.
        let _intrpt = InterruptGuard::new();
        // Lock scheduler for the current core.
        if let Some(que) = self.queue_mut(state) {
            que.push_back(thread);
        }
    }
}

/// Yields to a new thread while rescheduling the current thread to `new_state`.
///
/// This call blocks until the thread is switched back, or never returns if
/// `new_state` is [`ThreadState::Zombie`].
pub fn reschedule(new_state: ThreadState, intrpt: InterruptGuard) {
    if matches!(new_state, ThreadState::Running) {
        return;
    }
    if InterruptGuard::cnt() != 1 {
        error!("Entering sched::reschedule with more than 1 interrupt guard live.");
    }

    // Lock scheduler for the current core.
    let mut sched_lock = SCHED.lock();
    let Some(sched) = sched_lock.as_mut() else {
        return;
    };

    if sched.is_disabled {
        return;
    }

    // SAFETY: The access does not overlap with other access since we are holding
    // scheduler lock.
    let Some(mut new_thread) = sched.next_kthread() else {
        return;
    };

    let cur_meta = KThread::cur_meta(&intrpt);
    let cur_idle = KThread::cur_meta(&intrpt).priority == u8::MAX;
    let cpu_id = cur_meta.cpu_id;
    // FIXME: This is likely to still cause UB.
    // Note the cast here is appropriate because outside scheduler, there is no
    // reference to rsp.
    let cur_thread_rsp = (&raw const cur_meta.rsp).cast_mut();
    drop(cur_meta);

    new_thread.meta.cpu_id = cpu_id;
    let new_thread_rsp = new_thread.meta.rsp;

    let cur_thread = KThread::cur_thread_ptr();
    // SAFETY: All threads were allocated on stack.
    let cur_thread = unsafe { Box::from_raw(cur_thread) };

    if cur_idle {
        debug_assert!(sched.idle.is_none());
        sched.idle = Some(cur_thread);
    } else {
        sched.queue_mut(new_state).unwrap().push_back(cur_thread);
    }

    // We will get the memory back when new thread schedules itself back.
    Box::leak(new_thread);
    drop(sched_lock);

    // Note per-CPU structures are no longer for the current thread after switch_to.

    // SAFETY: cur and new are valid as guarenteed by before_switch. Interrupt and
    // scheduler are locked by before_switch.
    unsafe { switch_to(cur_thread_rsp, new_thread_rsp) };
    drop(intrpt);
}

/// Yield to another thread if available.
pub fn yield_kthread() {
    reschedule(
        ThreadState::Ready,
        InterruptGuard::new(),
    );
}

/// Entry point for a new `KThread`.
extern "C" fn kthread_entry() -> ! {
    // SAFETY: Jumping from switch_to.
    let intrpt = unsafe { InterruptGuard::reclaim() };
    if InterruptGuard::cnt() != 1 {
        error!("exiting switch_to with more than 1 interrupt guard live.");
    }
    let main = KThread::cur_meta(&intrpt).main;
    drop(intrpt);

    debug_assert!(InterruptGuard::cnt() == 0);
    main();

    // Exit by scheduling as zombie.
    reschedule(
        ThreadState::Zombie,
        InterruptGuard::new(),
    );

    // SAFETY: rescheduling to zombie will stop the thread from being switched to
    // again.
    unreachable!()
}

fn idle() {
    loop {
        ok!("idling!");
        hlt();
    }
}
fn idle1() {
    loop {
        ok!("idle1!");
        hlt();
    }
}
fn idle2() {
    loop {
        ok!("idle2!");
        hlt();
    }
}

pub enum ThreadState {
    Running,
    Ready,
    Zombie,
}
