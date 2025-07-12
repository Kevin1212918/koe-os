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

use pinned_init::{pin_data, pin_init_from_closure, InPlaceInit as _, PinInit};

use crate::arch::stack_ptr;
use crate::common::ll::{Link, Linked};
use crate::common::log::info;
use crate::common::StackPtr;
use crate::interrupt::IntrptGuard;
use crate::mem::addr::PageSize;
use crate::sched::arch::write_init_stack;

pub(super) const THREAD_LINK_OFFSET: usize = offset_of!(KStack, link);
pub const THREAD_PAGE_CNT: usize = 4;
pub const THREAD_SIZE: usize = THREAD_PAGE_CNT * PageSize::MIN.usize();
pub const KERNEL_STACK_SIZE: usize =
    THREAD_SIZE - size_of::<Link>() - size_of::<Tid>() - size_of::<StackPtr>();
const KERNEL_STACK_ARRAY_LEN: usize = KERNEL_STACK_SIZE / size_of::<usize>();


const _: () = assert!(size_of::<KStack>() == THREAD_SIZE);
const _: () = assert!(align_of::<KStack>() == THREAD_SIZE);

unsafe impl Linked<THREAD_LINK_OFFSET> for KStack {}
#[pin_data]
#[repr(C, align(16384))]
pub struct KStack {
    #[pin]
    _pin: PhantomPinned,
    #[pin]
    stack: [MaybeUninit<usize>; KERNEL_STACK_ARRAY_LEN],

    /// Unique thread ID.
    pub tid: Tid,

    /// Temporary storage for a thread's stack pointer while it is not running.
    /// Note that this is not accurate for a running thread.
    ///
    /// Used in context switch to switch stack.
    pub(super) sp: StackPtr,
    link: Link,
}
impl KStack {
    /// An inplace initializer for a new thread with the given stack. Note if
    /// the given stack is larger than `KERNEL_STACK_SIZE`, the excess is
    /// not used.
    ///
    /// The new stack need to be registered in scheduler before using.
    fn new() -> impl PinInit<Self, Infallible> {
        unsafe {
            pin_init_from_closure(move |slot: *mut Self| {
                let link = &raw mut (*slot).link;
                ptr::write(link, Link::new());

                let stack = &raw mut (*slot).stack;
                let stack = stack.as_mut_unchecked();
                let stack_len = write_init_stack(stack);
                debug_assert!(stack_len != 0);

                let stack_top = &raw mut (*slot).stack[KERNEL_STACK_ARRAY_LEN - stack_len] as usize;
                let sp = &raw mut (*slot).sp;
                ptr::write(sp, stack_top);

                let tid = &raw mut (*slot).tid;
                ptr::write(tid, Tid::new());

                Ok(())
            })
        }
    }

    pub fn boxed() -> Box<Self> {
        let new = Box::pin_init(KStack::new()).unwrap();

        // FIXME: Allow threads to store pinned box.
        // SAFETY:
        unsafe { Pin::into_inner_unchecked(new) }
    }

    pub fn cur_tid() -> Tid {
        let thread_ptr = Self::cur_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let tid_ptr = unsafe { &raw const (*thread_ptr).tid };

        // SAFETY: Tid is never modified.
        unsafe { ptr::read(tid_ptr) }
    }

    pub(super) fn cur_thread_ptr() -> *mut KStack {
        let stack_ptr = stack_ptr();

        let mask = !(THREAD_SIZE - 1);
        let thread_ptr = stack_ptr & mask;
        thread_ptr as *mut KStack
    }
}


#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct Tid(pub u64);
impl Tid {
    fn new() -> Tid {
        static NEXT_TID: AtomicU64 = AtomicU64::new(0);
        let res = NEXT_TID.fetch_add(1, Ordering::Relaxed);
        debug_assert!(res != u64::MAX);
        Self(res)
    }
}

impl Display for Tid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { self.0.fmt(f) }
}
