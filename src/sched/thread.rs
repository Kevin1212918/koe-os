use alloc::boxed::Box;
use alloc::sync::Arc;
use core::arch::asm;
use core::convert::Infallible;
use core::fmt::Display;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};
use core::pin::Pin;
use core::sync::atomic::{AtomicU64, Ordering};
use core::{ptr, u64};

use atomic::Atomic;
use pinned_init::{pin_data, pin_init_from_closure, InPlaceInit as _, PinInit};

use super::ThreadState;
use crate::arch::stack_ptr;
use crate::common::ll::{Link, Linked};
use crate::common::log::info;
use crate::common::StackPtr;
use crate::interrupt::IntrptGuard;
use crate::mem::addr::PageSize;
use crate::sched::arch::write_init_stack;

pub(super) const THREAD_LINK_OFFSET: usize = offset_of!(KThread, link);
pub const THREAD_PAGE_CNT: usize = 4;
pub const THREAD_SIZE: usize = THREAD_PAGE_CNT * PageSize::MIN.usize();
pub const KERNEL_STACK_SIZE: usize = THREAD_SIZE - size_of::<Link>() - size_of::<Tcb>();
const KERNEL_STACK_ARRAY_LEN: usize = KERNEL_STACK_SIZE / size_of::<usize>();


const _: () = assert!(size_of::<KThread>() == THREAD_SIZE);
const _: () = assert!(align_of::<KThread>() == THREAD_SIZE);

unsafe impl Linked<THREAD_LINK_OFFSET> for KThread {}
#[pin_data]
#[repr(C, align(16384))]
pub struct KThread {
    #[pin]
    _pin: PhantomPinned,
    #[pin]
    stack: [MaybeUninit<usize>; KERNEL_STACK_ARRAY_LEN],
    pub(super) tcb: Tcb,
    link: Link,
}
impl KThread {
    /// An inplace initializer for a new thread with the given stack. Note if
    /// the given stack is larger than `KERNEL_STACK_SIZE`, the excess is
    /// not used.
    ///
    /// The new stack need to be registered in scheduler before using.
    fn new(main: fn(), cpu_id: u8, priority: u8, is_usr: bool) -> impl PinInit<Self, Infallible> {
        unsafe {
            pin_init_from_closure(move |slot: *mut Self| {
                let link = &raw mut (*slot).link;
                ptr::write(link, Link::new());

                let stack = &raw mut (*slot).stack;
                let stack = stack.as_mut_unchecked();
                let stack_len = write_init_stack(stack);
                debug_assert!(stack_len != 0);

                let sp = &raw mut (*slot).stack[KERNEL_STACK_ARRAY_LEN - stack_len] as usize;
                let tid = ThreadId::new();

                let tcb = &raw mut (*slot).tcb;
                ptr::write(tcb, Tcb {
                    magic: Tcb::MAGIC,
                    tid,
                    sp,
                    main,
                    cpu_id,
                    priority,
                    is_usr,
                    state: Atomic::new(ThreadState::Ready),
                });

                Ok(())
            })
        }
    }

    pub(super) fn boxed(main: fn(), cpu_id: u8, priority: u8, is_usr: bool) -> Box<Self> {
        let res = Box::pin_init(KThread::new(
            main, cpu_id, priority, is_usr,
        ))
        .unwrap();
        // FIXME: Change dispatch to handle pinned kthread.
        // SAFETY: Unsafe!
        unsafe { Pin::into_inner_unchecked(res) }
    }

    pub fn stack_base(&self) -> StackPtr {
        unsafe { (&raw const self.stack).add(KERNEL_STACK_ARRAY_LEN) as StackPtr }
    }

    pub fn my_tid() -> ThreadId {
        let thread_ptr = Self::my_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let tid_ptr = unsafe { &raw const (*thread_ptr).tcb.tid };

        // SAFETY: Tid is always immutable.
        unsafe { ptr::read(tid_ptr) }
    }

    pub fn my_cpu_id() -> u8 {
        let thread_ptr = Self::my_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let cpu_id_ptr = unsafe { &raw const (*thread_ptr).tcb.cpu_id };

        // SAFETY: CPU ID is only modified when the current thread is moved across the
        // CPU. This will never occur when the current thread is running, so cpu
        // id is immutable while the current thread is running.
        unsafe { ptr::read(cpu_id_ptr) }
    }

    pub(super) fn my_main() -> fn() {
        let thread_ptr = Self::my_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let main_ptr = unsafe { &raw const (*thread_ptr).tcb.main };

        // SAFETY: main is always immutable.
        unsafe { ptr::read(main_ptr) }
    }

    pub(super) fn my_thread_ptr() -> *mut KThread {
        let stack_ptr = stack_ptr();

        let mask = !(THREAD_SIZE - 1);
        let thread_ptr = stack_ptr & mask;
        thread_ptr as *mut KThread
    }
}

#[repr(C)]
pub struct Tcb {
    /// Magic value to check kernel stack overflow.
    pub magic: u64,

    /// Unique thread ID.
    pub tid: ThreadId,

    /// Temporary storage for a thread's stack pointer while it is not running.
    /// Note that this is not accurate for a running thread.
    ///
    /// Used in context switch to switch stack.
    pub(super) sp: StackPtr,

    /// Main function of the thread. This is used during initialization.
    pub main: fn(),

    /// CPU ID.
    pub cpu_id: u8,

    /// Priority of the thread. Lower value is higher priority.
    pub priority: u8,

    /// Does the kthread back a user task.
    pub is_usr: bool,

    /// Current execution state
    pub state: Atomic<ThreadState>,
}

impl Tcb {
    const MAGIC: u64 = 0xBCDBBAAA;
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct ThreadId(pub u64);
impl ThreadId {
    fn new() -> ThreadId {
        static NEXT_TID: AtomicU64 = AtomicU64::new(0);
        let res = NEXT_TID.fetch_add(1, Ordering::Relaxed);
        debug_assert!(res != u64::MAX);
        Self(res)
    }
}

impl Display for ThreadId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { self.0.fmt(f) }
}
