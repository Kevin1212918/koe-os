use alloc::boxed::Box;
use core::alloc::Allocator;
use core::arch::asm;
use core::convert::Infallible;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};
use core::pin::Pin;
use core::ptr::metadata;
use core::{ptr, slice, u64};

use pinned_init::{
    init_from_closure, pin_data, pin_init, pin_init_from_closure, InPlaceInit as _, PinInit,
};

use super::kthread_entry;
use crate::common::ll::{Link, Linked};
use crate::interrupt::IntrptGuard;
use crate::mem::addr::PageSize;
use crate::sched::arch::write_init_stack;

pub(super) const THREAD_LINK_OFFSET: usize = offset_of!(KThread, link);
pub const THREAD_PAGE_CNT: usize = 2;
pub const THREAD_SIZE: usize = THREAD_PAGE_CNT * PageSize::MIN.usize();
pub const KERNEL_STACK_SIZE: usize = THREAD_SIZE - size_of::<Link>() - size_of::<Metadata>();
const KERNEL_STACK_ARRAY_LEN: usize = KERNEL_STACK_SIZE / size_of::<usize>();


const _: () = assert!(size_of::<KThread>() == THREAD_SIZE);
const _: () = assert!(align_of::<KThread>() == THREAD_SIZE);

unsafe impl Linked<THREAD_LINK_OFFSET> for KThread {}
#[pin_data]
#[repr(C, align(8192))]
pub struct KThread {
    #[pin]
    _pin: PhantomPinned,
    #[pin]
    stack: [MaybeUninit<usize>; KERNEL_STACK_ARRAY_LEN],
    pub(super) meta: Metadata,
    link: Link,
}
impl KThread {
    /// An inplace initializer for a new thread with the given stack. Note if
    /// the given stack is larger than `KERNEL_STACK_SIZE`, the excess is
    /// not used.
    pub fn new(main: fn(), priority: u8) -> impl PinInit<Self, Infallible> {
        unsafe {
            pin_init_from_closure(move |slot: *mut Self| {
                let link = &raw mut (*slot).link;
                ptr::write(link, Link::new());

                let stack = &raw mut (*slot).stack;
                let stack = stack.as_mut_unchecked();
                let stack_len = write_init_stack(stack);
                debug_assert!(stack_len != 0);

                let rsp = &raw mut (*slot).stack[KERNEL_STACK_ARRAY_LEN - stack_len] as usize;
                let meta = &raw mut (*slot).meta;
                ptr::write(meta, Metadata {
                    is_usr: false,
                    cpu_id: 0,
                    rsp,
                    main,
                    priority,
                });

                Ok(())
            })
        }
    }

    pub fn boxed(main: fn(), priority: u8) -> Box<Self> {
        let new = Box::pin_init(KThread::new(main, priority)).unwrap();

        // FIXME: Allow threads to store pinned box.
        // SAFETY:
        unsafe { Pin::into_inner_unchecked(new) }
    }

    pub fn cur_meta(_intrpt: &IntrptGuard) -> &Metadata {
        let thread_ptr = Self::cur_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let meta_ptr = unsafe { &raw const (*thread_ptr).meta };

        // SAFETY: Caller guarentees the scheduler that manages the current thread is
        // not running, so all other accesses to the thread metadata is through
        // this function, which grants shared reference.
        unsafe { meta_ptr.as_ref_unchecked() }
    }

    /// # Safety
    /// Caller should ensure there are no live reference to the metadata.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn cur_meta_mut(_intrpt: &IntrptGuard) -> &mut Metadata {
        let thread_ptr = Self::cur_thread_ptr();
        // SAFETY: dereference in place expr is safe.
        let meta_ptr = unsafe { &raw mut (*thread_ptr).meta };

        // SAFETY: Caller guarentees no other reference to metadata exists. As long as
        // interrupt is disabled, the reference will not be leaked.
        unsafe { meta_ptr.as_mut_unchecked() }
    }

    pub(super) fn cur_thread_ptr() -> *mut KThread {
        let stack_ptr: u64;

        // SAFETY: Reading stack pointer is safe.
        unsafe { asm!("mov r11, rsp", out("r11") stack_ptr ) };

        let mask = !(THREAD_SIZE as u64 - 1);
        let thread_ptr = stack_ptr & mask;
        thread_ptr as *mut KThread
    }
}

pub struct Metadata {
    /// Is the thread backed by a userspace.
    pub is_usr: bool,
    /// CPU ID
    pub cpu_id: u8,
    /// Temporary storage for a thread's stack pointer while it is not running.
    /// Note that this is not accurate for a running thread.
    ///
    /// Used in context switch to switch stack.
    pub(super) rsp: usize,
    /// Main function of the thread. This is used during initialization.
    pub(super) main: fn(),
    /// Priority of the thread. Lower value is higher priority.
    pub(super) priority: u8,
}
