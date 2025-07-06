use core::arch::asm;
use core::convert::Infallible;
use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};
use core::ptr::metadata;
use core::{ptr, slice, u64};

use pinned_init::{init_from_closure, pin_data, pin_init, pin_init_from_closure, PinInit};

use crate::common::ll::{Link, Linked};
use crate::interrupt::InterruptGuard;
use crate::mem::addr::PageSize;

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
    pub(super) meta: Metadata,
    link: Link,
    #[pin]
    stack: [MaybeUninit<usize>; KERNEL_STACK_ARRAY_LEN],
    #[pin]
    _pin: PhantomPinned,
}
impl KThread {
    /// An inplace initializer for a new thread with the given stack. Note if
    /// the given stack is larger than `KERNEL_STACK_SIZE`, the excess is
    /// not used.
    pub fn new(
        init_stack: &[MaybeUninit<usize>],
        main: fn(),
        priority: u8,
    ) -> impl PinInit<Self, Infallible> + '_ {
        unsafe {
            pin_init_from_closure(move |slot: *mut Self| {
                let link = &raw mut (*slot).link;
                ptr::write(link, Link::new());

                let stack = &raw mut (*slot).stack;
                let stack = stack.as_mut_unchecked();

                let copy_len = stack.len().min(init_stack.len());
                stack[0..copy_len].copy_from_slice(&init_stack[0..copy_len]);

                let rsp = &raw mut (*slot).stack[copy_len] as usize;
                // Offset rsp by 1 so it is pointing at the
                // last element.
                let rsp = rsp - size_of::<usize>();

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


    pub fn cur_meta(_intrpt: &InterruptGuard) -> &Metadata {
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
    pub unsafe fn cur_meta_mut(_intrpt: &InterruptGuard) -> &mut Metadata {
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
    /// Entry point of the thread. This is used during initialization.
    pub(super) main: fn(),
    /// Priority of the thread. Lower value is higher priority.
    pub(super) priority: u8,
}
