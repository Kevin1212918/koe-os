use alloc::alloc::Global;
use alloc::boxed::Box;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use arch::switch_to;
use atomic::Atomic;
use bytemuck::NoUninit;
use hashbrown::HashMap;
use thread::THREAD_LINK_OFFSET;

use crate::arch::hlt;
use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::LinkedList;
use crate::common::log::{error, info, ok};
use crate::common::StackPtr;
use crate::interrupt::IntrptGuard;
use crate::sync::{spin, InitCell};

mod arch;
mod thread;

pub use thread::{KThread, ThreadId};

static THREAD_MAP: InitCell<spin::Mutex<ThreadMap>> = unsafe { InitCell::new() };
static DISPATCHERS: InitCell<[spin::Mutex<Dispatcher>; 1]> = unsafe { InitCell::new() };

/// Initialize scheduler and schedule the main task to be run later.
pub fn init_scheduler(main: fn()) {
    let sched = Scheduler::new();
    sched.launch(main, 1);
}

pub fn init_switch() -> ! {
    let intrpt = IntrptGuard::new();
    //  SAFETY: Reclaiming the initial preempt guard. Note that since intrpt guard
    // is live, no preemption will occur until intrpt is enabled.
    drop(unsafe { PreemptGuard::reclaim() });
    // SAFETY: Initial threads are on cpu 0
    unsafe { Scheduler::new().force_switch(0, Some(intrpt)) };
}

/// Per-CPU structure managing currently running threads.
pub struct Scheduler();
impl Scheduler {
    pub fn new() -> Self {
        static SCHED: spin::Once<()> = spin::Once::new();
        SCHED.call_once(|| {
            // SAFETY: SCHED is called only once.
            unsafe {
                THREAD_MAP.init(spin::Mutex::new(ThreadMap(
                    HashMap::new(),
                )));
                DISPATCHERS.init([spin::Mutex::new(Dispatcher::new())]);
            }
            let sched = Scheduler();
            sched.launch(idle, u8::MAX);
        });
        Self()
    }

    /// Create a new thread and schedule it as `ThreadState::Ready`.
    ///
    /// Returns the `Tid` to the new thread.
    pub fn launch(&self, main: fn(), priority: u8) -> ThreadId {
        let preempt = PreemptGuard::new();
        let thread = KThread::boxed(main, 0, priority, false);
        let tid = thread.tcb.tid;
        info!("Launch thread {}", tid);

        let mut thread_map = THREAD_MAP.lock();
        debug_assert!(!thread_map.0.contains_key(&tid));
        let (_, tcb) = unsafe {
            thread_map
                .0
                .insert_unique_unchecked(tid, &raw const *thread)
        };

        let mut dispatch = DISPATCHERS[0].lock();
        drop(thread_map);

        dispatch.put(thread);
        tid
    }

    // TODO: refactor reschedule and force_switch

    /// Yields to a new thread while rescheduling the current thread to
    /// `new_state`.
    ///
    /// This call blocks until the thread is switched back, or never returns if
    /// `new_state` is [`ThreadState::Zombie`].
    pub fn reschedule(&self, new_state: ThreadState, intrpt: Option<IntrptGuard>) {
        if new_state == ThreadState::Running {
            return;
        }
        if IntrptGuard::cnt() > 1 || intrpt.is_none() && IntrptGuard::cnt() > 0 {
            error!("Entering sched::reschedule with untracked interrupt guards.");
        }
        let intrpt = intrpt.unwrap_or_else(|| IntrptGuard::new());
        let mut dispatch = DISPATCHERS[0].lock();

        let Some(mut new_thread) = dispatch.next() else {
            return;
        };

        let new_tid = new_thread.tcb.tid;
        let new_tcb = &mut new_thread.tcb;

        debug_assert!(new_tcb.state.load(Ordering::Relaxed) == ThreadState::Ready);
        new_tcb.state.store(ThreadState::Running, Ordering::Relaxed);
        let new_stack_ptr = new_tcb.sp;
        // NOTE: New running thread is recovered next time it enters reschedule.
        Box::leak(new_thread);

        // SAFETY: The current thread is in the current dispatcher, which is locked.
        let mut my_thread = unsafe { Box::from_raw(KThread::my_thread_ptr()) };
        let my_tcb = &mut my_thread.tcb;
        let my_tid = my_tcb.tid;

        debug_assert!(my_tcb.state.load(Ordering::Relaxed) == ThreadState::Running);
        my_tcb.state.store(new_state, Ordering::Relaxed);
        let my_stack_ptr = &raw mut my_tcb.sp;
        dispatch.put(my_thread);

        // NOTE: Dispatch is reclaimed when exiting switch_to
        spin::MutexGuard::leak(dispatch);
        // NOTE: IntrptGuard is reclaimed when exiting switch_to
        intrpt.leak();
        info!(
            "Switch from thread {} to thread {}",
            my_tid, new_tid
        );

        // SAFETY: cur and new are valid as guarenteed by before_switch. Interrupt and
        // scheduler are locked by before_switch.
        unsafe { switch_to(my_stack_ptr, new_stack_ptr) };

        // SAFETY: reclaiming previously leaked intrpt guard.
        unsafe { IntrptGuard::reclaim() };

        // SAFETY: unlock current dispatcher.
        unsafe { DISPATCHERS[KThread::my_cpu_id() as usize].force_unlock() };
    }

    /// Transfer control to a ready thread and leak the current thread.
    ///
    /// This function does not assume the running stack is from a `KThread`, so
    /// requires the correct `cpu_id` to be provided.
    ///
    /// # Safety
    /// - `cpu_id` should specifiy the currently running CPU.
    pub unsafe fn force_switch(&self, cpu_id: u8, intrpt: Option<IntrptGuard>) -> ! {
        // NOTE: IntrptGuard is reclaimed when exiting switch_to
        let intrpt = intrpt.unwrap_or_else(|| IntrptGuard::new());
        let mut dispatch = DISPATCHERS[cpu_id as usize].lock();

        let Some(mut new_thread) = dispatch.next() else {
            panic!("No runnable thread found after init.");
        };
        let new_tcb = &mut new_thread.tcb;
        info!("Force switch to thread {}", new_tcb.tid);

        debug_assert!(new_tcb.state.load(Ordering::Relaxed) == ThreadState::Ready);
        new_tcb.state.store(ThreadState::Running, Ordering::Relaxed);
        let new_stack_ptr = new_tcb.sp;
        // NOTE: New running thread is recovered next time it enters reschedule.
        Box::leak(new_thread);

        // NOTE: Dispatch is reclaimed when exiting switch_to
        spin::MutexGuard::leak(dispatch);
        // NOTE: IntrptGuard is reclaimed when exiting switch_to
        intrpt.leak();

        let mut dummy: StackPtr = 0;
        // SAFETY: cur and new are valid as guarenteed by before_switch. Interrupt and
        // scheduler are locked by before_switch.
        unsafe { switch_to(&raw mut dummy, new_stack_ptr) };
        unreachable!()
    }
}

/// Entry point for a new `KThread`.
extern "C" fn kthread_entry() -> ! {
    if IntrptGuard::cnt() != 1 {
        error!("Exiting switch_to with incorrect count of interrupt guard.");
    }
    // SAFETY: reclaiming previously leaked intrpt guard.
    let intrpt = unsafe { IntrptGuard::reclaim() };
    drop(intrpt);

    // SAFETY: unlock current dispatcher.
    unsafe { DISPATCHERS[KThread::my_cpu_id() as usize].force_unlock() };

    debug_assert!(IntrptGuard::cnt() == 0);
    KThread::my_main()();

    // Exit by scheduling as zombie.
    Scheduler::new().reschedule(ThreadState::Zombie, None);

    // SAFETY: rescheduling to zombie will stop the thread from being switched to
    // again.
    unreachable!()
}

struct Dispatcher {
    ready_threads: LinkedList<THREAD_LINK_OFFSET, Box<KThread>>,
    zombie_threads: LinkedList<THREAD_LINK_OFFSET, Box<KThread>>,
    idle: Option<Box<KThread>>,
}
impl Dispatcher {
    fn new() -> Self {
        Self {
            ready_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KThread>>::new_in(Global),
            zombie_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KThread>>::new_in(Global),
            idle: None,
        }
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

    /// Take the next ready thread.
    fn next(&mut self) -> Option<Box<KThread>> {
        self.ready_threads
            .front_mut()
            .remove()
            .or_else(|| self.idle.take())
    }

    /// Puts the thread on a queue.
    ///
    /// If the thread is an idle thread, it is put in the idle slot.
    fn put(&mut self, thread: Box<KThread>) {
        if thread.tcb.priority != u8::MAX {
            let que = self
                .queue_mut(thread.tcb.state.load(Ordering::Relaxed))
                .unwrap();
            que.push_back(thread);
            return;
        }
        if self.idle.is_some() {
            error!("Parking an idle thread when one exists.");
            return;
        }
        self.idle.insert(thread);
    }
}

unsafe impl Send for ThreadMap {}
struct ThreadMap(HashMap<ThreadId, *const KThread>);

/// An RAII implementation of reentrant preempt lock. This structure
/// guarentees that no preemption will occur on the CPU.
pub struct PreemptGuard();
impl PreemptGuard {
    pub fn new() -> Self {
        PREEMPT_GUARD_CNT.fetch_add(1, atomic::Ordering::Relaxed);
        Self()
    }
    /// # Safety
    /// `reclaim` should always correspond to a previously leaked guard.
    pub unsafe fn reclaim() -> Self { Self() }

    pub fn leak(self) { core::mem::forget(self) }
    pub fn cnt() -> usize { PREEMPT_GUARD_CNT.load(atomic::Ordering::Relaxed) }
}

impl Drop for PreemptGuard {
    fn drop(&mut self) { PREEMPT_GUARD_CNT.fetch_sub(1, atomic::Ordering::Relaxed); }
}

/// Per-CPU tracker for the number of preempt guard in the kernel.
///
/// Note preemption starts enabled.
static PREEMPT_GUARD_CNT: AtomicUsize = AtomicUsize::new(1);

/// Reschedules to another thread if preemption is enabled.
pub fn preempt(intrpt: IntrptGuard) {
    if PreemptGuard::cnt() == 0 {
        // SAFETY: Preemption is enabled after scheduler is initialized.
        Scheduler::new().reschedule(ThreadState::Ready, Some(intrpt));
    }
}



fn idle() {
    loop {
        ok!("Idling...");
        hlt();
    }
}

unsafe impl NoUninit for ThreadState {}
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Ready,
    Zombie,
}
