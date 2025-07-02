use alloc::alloc::Global;
use alloc::boxed::Box;
use core::arch::global_asm;
use core::cell::{SyncUnsafeCell, UnsafeCell};
use core::hint::unreachable_unchecked;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};

use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::interrupt::InterruptGuard;
use crate::mem::addr::PageSize;
use crate::mem::{PageAllocator, PhysicalRemapSpace, UserSpace};

static SCHED: spin::Mutex<spin::Lazy<Scheduler>> =
    spin::Mutex::new(spin::Lazy::new(|| Scheduler::new()));

// switch_to:
// push callee-saved
// record rsp
// switch rsp
// pop callee-saved
// ret

const THREAD_LINK_OFFSET: usize = offset_of!(Thread, link);
const THREAD_PAGE_CNT: usize = 2;
const THREAD_SIZE: usize = THREAD_PAGE_CNT * PageSize::MIN.usize();
const KERNEL_STACK_SIZE: usize =
    THREAD_SIZE - size_of::<Link>() - size_of::<u64>() - size_of::<PhantomPinned>();
const _: () = assert!(size_of::<Thread>() == THREAD_SIZE);

unsafe impl Linked<THREAD_LINK_OFFSET> for Thread {}
#[repr(C, align(8192))]
struct Thread {
    link: Link,
    rsp: u64,
    _pin: PhantomPinned,

    stack: [MaybeUninit<u8>; KERNEL_STACK_SIZE],
}

/// Per-CPU structure managing currently running threads.
struct SchedulerRecord {
    running_thread: Option<Box<Thread>>,
    ready_threads: LinkedList<THREAD_LINK_OFFSET, Box<Thread>>,
    zombie_threads: LinkedList<THREAD_LINK_OFFSET, Box<Thread>>,
    idle: Option<Box<Thread>>,
}
struct Scheduler(spin::Mutex<SchedulerRecord>);
impl Scheduler {
    fn new(idle: Box<Thread>) -> Self {
        Scheduler(spin::Mutex::new(SchedulerRecord {
            running_thread: None,
            ready_threads: LinkedList::<THREAD_LINK_OFFSET, Box<Thread>>::new_in(Global),
            zombie_threads: LinkedList::<THREAD_LINK_OFFSET, Box<Thread>>::new_in(Global),
            idle: Some(idle),
        }))
    }

    /// Yield to another thread if available.
    pub fn yield_thread(&self) { self.reschedule(ThreadState::Ready); }

    fn reschedule(&self, new_state: ThreadState) {
        if matches!(new_state, ThreadState::Running) {
            // We are already running.
            return;
        }

        let intrpt_guard = InterruptGuard::new();
        let sched_guard = spin::MutexGuard::leak(self.0.lock());
        let new_thread_rsp = {
            // SAFETY: The access does not overlap with other access since we are holding
            // scheduler lock.
            let new_thread_opt = sched_guard
                .ready_threads
                .front()
                .get()
                .or_else(|| sched_guard.idle.as_deref());

            let Some(new_thread) = new_thread_opt else {
                // Idle is empty, so the current thread is idle.
                return;
            };
            new_thread.rsp
        };

        let mut cur_thread = sched_guard.running_thread.take().unwrap();
        let que = match new_state {
            ThreadState::Ready => &mut sched_guard.ready_threads,
            ThreadState::Zombie => &mut sched_guard.zombie_threads,
            // SAFETY: we check at start of the function that new_state is not running.
            ThreadState::Running => unsafe { unreachable_unchecked() },
        };
        let cur_thread_rsp = &raw mut cur_thread.rsp;
        let cur_thread_ptr = &raw const *cur_thread;
        que.push_back(cur_thread);

        drop(que);
        drop(new_state);

        unsafe { switch_to(cur_thread_rsp, new_thread_rsp) };

        let mut cur_thread = unsafe {
            sched_guard
                .ready_threads
                .cursor_mut_from_ptr(cur_thread_ptr)
        };
        let cur_thread = unsafe { cur_thread.remove().unwrap_unchecked() };
        debug_assert!(sched_guard.running_thread.is_none());
        sched_guard.running_thread.replace(cur_thread);

        unsafe { self.0.force_unlock() };
    }
}

enum ThreadState {
    Running,
    Ready,
    Zombie,
}

global_asm!(include_str!("sched/switch.S"));
unsafe extern "C" {
    /// Switch thread to the thread with `new_rsp`.
    ///
    /// This function blocks until the current thread is switched back.
    ///
    /// # Safety
    /// - `old_rsp` should point to the currently executing `Thread`'s `rsp`
    ///   field.
    /// - `new_rsp` should point to top of new `Thread`'s stack.
    /// - Caller should hold an [`InterruptGuard`] until this function returns.
    unsafe fn switch_to(old_rsp: *mut u64, new_rsp: u64);
}
