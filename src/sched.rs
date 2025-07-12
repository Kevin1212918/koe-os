use alloc::alloc::Global;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicUsize, Ordering};

use arch::switch_to;
use atomic::Atomic;
use bytemuck::NoUninit;
use hashbrown::HashMap;
use thread::{Tid, THREAD_LINK_OFFSET};

use crate::arch::hlt;
use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::LinkedList;
use crate::common::log::{error, info, ok};
use crate::common::StackPtr;
use crate::interrupt::IntrptGuard;
use crate::sync::spin;

mod arch;
mod thread;

pub use thread::KStack;

pub static SCHED: spin::Mutex<Option<Scheduler>> = spin::Mutex::new(None);

/// Initialize scheduler and schedule the main task to be run later.
pub fn init_scheduler(main: fn()) {
    let mut sched_slot = SCHED.lock();
    let sched = sched_slot.insert(Scheduler::new());
    sched.launch_thread(main, 1);
}

pub fn init_switch() -> ! {
    let intrpt = IntrptGuard::new();
    // Reclaiming the initial preempt guard.
    drop(unsafe { PreemptGuard::reclaim() });
    Scheduler::force_switch(Some(intrpt));
}

/// Per-CPU structure managing currently running threads.
pub struct Scheduler {
    thread_map: HashMap<Tid, Tcb>,
    dispatchers: [Dispatcher; 1],
}
impl Scheduler {
    fn new() -> Self {
        let mut ret = Self {
            thread_map: HashMap::new(),
            dispatchers: [Dispatcher::new()],
        };

        ret.launch_thread(idle, u8::MAX);
        ret
    }

    fn launch_thread(&mut self, main: fn(), priority: u8) -> Tid {
        let thread = KStack::boxed();
        let tid = thread.tid;
        let tcb = Tcb {
            is_usr: false,
            cpu_id: 0,
            main,
            priority,
            kstack: &raw const *thread,
            state: Atomic::new(ThreadState::Ready),
        };
        let dispatch = &mut self.dispatchers[0];
        info!("Launch thread {}", tid);
        debug_assert!(!self.thread_map.contains_key(&tid));
        // FIXME: This seems to trigger UB in allocator. Use the try_insert version for
        // now. let (_, tcb) = unsafe {
        // self.thread_map.insert_unique_unchecked(tid, tcb) };
        let tcb = self.thread_map.try_insert(tid, tcb).unwrap();
        dispatch.put(tcb, thread);

        tid
    }

    /// Create a new thread and schedule it as `ThreadState::Ready`.
    ///
    /// Returns the `Tid` to the new thread.
    pub fn launch(main: fn(), priority: u8) -> Tid {
        let mut sched = SCHED.lock();
        let Some(sched) = sched.as_mut() else {
            // SAFETY: unlock the sched from above.
            unsafe { SCHED.force_unlock() };
            panic!("Scheduler is not initialized");
        };

        let ret = sched.launch_thread(main, priority);
        ret
    }

    // TODO: refactor reschedule and force_switch

    /// Yields to a new thread while rescheduling the current thread to
    /// `new_state`.
    ///
    /// This call blocks until the thread is switched back, or never returns if
    /// `new_state` is [`ThreadState::Zombie`].
    pub fn reschedule(new_state: ThreadState, intrpt: Option<IntrptGuard>) {
        if new_state == ThreadState::Running {
            return;
        }

        // NOTE: Scheduler lock is reclaimed when exiting switch_to.
        let sched = spin::MutexGuard::leak(SCHED.lock());
        let Some(sched) = sched.as_mut() else {
            // SAFETY: unlock the sched from above.
            unsafe { SCHED.force_unlock() };
            error!("Scheduler is not initialized");
            return;
        };

        if IntrptGuard::cnt() > 1 || intrpt.is_none() && IntrptGuard::cnt() > 0 {
            error!("Entering sched::reschedule with untracked interrupt guards.");
        }

        let dispatch = &mut sched.dispatchers[0];

        let Some(new_thread) = dispatch.next() else {
            return;
        };
        let new_tid = new_thread.tid;
        let new_tcb = sched.thread_map.get(&new_tid).unwrap();

        debug_assert!(new_tcb.state.load(Ordering::Relaxed) == ThreadState::Ready);
        new_tcb.state.store(ThreadState::Running, Ordering::Relaxed);
        let new_stack_ptr = new_thread.sp;
        // NOTE: New running thread is recovered next time it enters reschedule.
        Box::leak(new_thread);


        let my_tid = KStack::cur_tid();
        let my_tcb = sched.thread_map.get(&my_tid).unwrap();

        debug_assert!(my_tcb.state.load(Ordering::Relaxed) == ThreadState::Running);
        my_tcb.state.store(new_state, Ordering::Relaxed);
        let mut my_thread = unsafe { Box::from_raw(KStack::cur_thread_ptr()) };
        let my_stack_ptr = &raw mut my_thread.sp;
        dispatch.put(my_tcb, my_thread);

        info!(
            "Switch from thread {} to thread {}",
            my_tid, new_tid
        );
        // NOTE: IntrptGuard is reclaimed when exiting switch_to
        intrpt.unwrap_or_else(|| IntrptGuard::new()).leak();

        // SAFETY: cur and new are valid as guarenteed by before_switch. Interrupt and
        // scheduler are locked by before_switch.
        unsafe { switch_to(my_stack_ptr, new_stack_ptr) };

        // SAFETY: reclaiming previously leaked intrpt guard.
        unsafe { IntrptGuard::reclaim() };
        // SAFETY: unlock the sched from beginning.
        unsafe {
            SCHED.force_unlock();
        }
    }

    /// Transfer control to a ready thread and leak the current thread.
    ///
    /// # Panic
    /// This will panic if scheduler is not initialized or there is no runnable
    /// thread.
    pub fn force_switch(intrpt: Option<IntrptGuard>) -> ! {
        // NOTE: Scheduler lock is reclaimed when exiting switch_to.
        let sched = spin::MutexGuard::leak(SCHED.lock());
        let Some(sched) = sched.as_mut() else {
            // SAFETY: unlock the sched from above.
            unsafe { SCHED.force_unlock() };
            panic!("Scheduler is not initialized");
        };

        let dispatch = &mut sched.dispatchers[0];

        let Some(new_thread) = dispatch.next() else {
            panic!("No runnable thread found after init.");
        };
        let new_tid = new_thread.tid;
        let new_tcb = sched.thread_map.get(&new_tid).unwrap();

        debug_assert!(new_tcb.state.load(Ordering::Relaxed) == ThreadState::Ready);
        new_tcb.state.store(ThreadState::Running, Ordering::Relaxed);
        let new_stack_ptr = new_thread.sp;
        // NOTE: New running thread is recovered next time it enters reschedule.
        Box::leak(new_thread);

        info!("Force switch to thread {}", new_tid);

        let mut dummy: StackPtr = 0;
        // NOTE: IntrptGuard is reclaimed when exiting switch_to
        intrpt.unwrap_or_else(|| IntrptGuard::new()).leak();

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
    // SAFETY: sched is locked from switch_to from beginning.
    let sched = unsafe {
        SCHED
            .as_mut_ptr()
            .as_mut_unchecked()
            .as_mut()
            .unwrap_unchecked()
    };
    let tid = KStack::cur_tid();
    let main = sched.thread_map.get(&tid).unwrap().main;

    drop(sched);
    // SAFETY: the mutable reference is dropped. Unlocking the sched lock from
    // before switch_to.
    unsafe { SCHED.force_unlock() };
    drop(intrpt);

    debug_assert!(IntrptGuard::cnt() == 0);
    main();

    // Exit by scheduling as zombie.
    Scheduler::reschedule(ThreadState::Zombie, None);

    // SAFETY: rescheduling to zombie will stop the thread from being switched to
    // again.
    unreachable!()
}


struct Dispatcher {
    ready_threads: LinkedList<THREAD_LINK_OFFSET, Box<KStack>>,
    zombie_threads: LinkedList<THREAD_LINK_OFFSET, Box<KStack>>,
    idle: Option<Box<KStack>>,
}
impl Dispatcher {
    fn new() -> Self {
        Self {
            ready_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KStack>>::new_in(Global),
            zombie_threads: LinkedList::<THREAD_LINK_OFFSET, Box<KStack>>::new_in(Global),
            idle: None,
        }
    }

    /// Return `None` if scheduler does not keep track of the state.
    fn queue_mut(
        &mut self,
        state: ThreadState,
    ) -> Option<&mut LinkedList<THREAD_LINK_OFFSET, Box<KStack>>> {
        match state {
            ThreadState::Ready => Some(&mut self.ready_threads),
            ThreadState::Zombie => Some(&mut self.zombie_threads),
            // SAFETY: we check at start of the function that new_state is not running.
            ThreadState::Running => None,
        }
    }

    /// Take the next ready thread.
    fn next(&mut self) -> Option<Box<KStack>> {
        self.ready_threads
            .front_mut()
            .remove()
            .or_else(|| self.idle.take())
    }

    /// Puts the thread on a queue.
    ///
    /// If the thread is an idle thread, it is put in the idle slot.
    fn put(&mut self, tcb: &Tcb, thread: Box<KStack>) {
        if tcb.priority != u8::MAX {
            let que = self.queue_mut(tcb.state.load(Ordering::Relaxed)).unwrap();
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
        Scheduler::reschedule(ThreadState::Ready, Some(intrpt));
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

unsafe impl Send for Tcb {}
#[derive(Debug)]
struct Tcb {
    /// Is the thread backed by a userspace.
    pub is_usr: bool,
    /// CPU ID
    pub cpu_id: u8,
    /// Main function of the thread. This is used during initialization.
    main: fn(),
    /// Priority of the thread. Lower value is higher priority.
    priority: u8,
    /// Pointer to `KStack`
    kstack: *const KStack,
    /// Current execution state
    state: Atomic<ThreadState>,
}
