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
                stack[KERNEL_STACK_ARRAY_LEN - copy_len..KERNEL_STACK_ARRAY_LEN]
                    .copy_from_slice(&init_stack[0..copy_len]);

                let rsp = &raw mut (*slot).stack[KERNEL_STACK_ARRAY_LEN - copy_len] as usize;
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
        let init_stack = INIT_KTHREAD_STACK.as_uninit_usizes();
        let new = Box::pin_init(KThread::new(init_stack, main, priority)).unwrap();

        // FIXME: Allow threads to store pinned box.
        // SAFETY:
        unsafe { Pin::into_inner_unchecked(new) }
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


/// Initial `KThread` stack.
///
/// When building a new `KThread`, `INIT_KTHREAD_STACK` will be byte-wise copied
/// to the new thread's address aligned stack.
#[repr(C)]
pub(super) struct InitKThreadStack {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    kthread_entry: extern "C" fn() -> !,
    null: u64, // This is here to fix kthread_entry alignment.
}
impl InitKThreadStack {
    pub fn as_uninit_usizes(&self) -> &[MaybeUninit<usize>] {
        let base: *const MaybeUninit<usize> = (self as *const InitKThreadStack).cast();
        let len = size_of::<InitKThreadStack>() / size_of::<usize>();
        const _: () = assert!(size_of::<InitKThreadStack>() % size_of::<usize>() == 0);
        const _: () = assert!(align_of::<InitKThreadStack>() <= align_of::<usize>());

        // SAFETY:
        // base is not null since it comes from `self`.
        // base is aligned to usize since align of InitKThreadStack is smaller than that
        // of usize. the memory range pointed is len * size_of(usize), which
        // equals size_of(InitKThreadStack).
        // MaybeUninit is always initialized.
        // Lifetime inherited from `self` prevents mutation for duration of the
        // lifetime.
        // InitiKThreadStack size does not overflow.
        unsafe { slice::from_raw_parts(base, len) }
    }
}

pub(super) static INIT_KTHREAD_STACK: InitKThreadStack = InitKThreadStack {
    r15: 0,
    r14: 0,
    r13: 0,
    r12: 0,
    rbx: 0,
    rbp: 0,
    kthread_entry,
    null: 0,
};
