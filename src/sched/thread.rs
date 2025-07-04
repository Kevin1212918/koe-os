use core::marker::PhantomPinned;
use core::mem::{offset_of, MaybeUninit};

use pinned_init::pin_data;

use crate::common::ll::{Link, Linked};
use crate::mem::addr::PageSize;

pub(super) const THREAD_LINK_OFFSET: usize = offset_of!(Thread, link);
pub const THREAD_PAGE_CNT: usize = 2;
pub const THREAD_SIZE: usize = THREAD_PAGE_CNT * PageSize::MIN.usize();
pub const KERNEL_STACK_SIZE: usize = THREAD_SIZE - size_of::<Link>() - size_of::<Metadata>();
const _: () = assert!(size_of::<Thread>() == THREAD_SIZE);

unsafe impl Linked<THREAD_LINK_OFFSET> for Thread {}
#[pin_data]
#[repr(C, align(8192))]
pub struct Thread {
    link: Link,
    pub meta: Metadata,
    #[pin]
    _pin: PhantomPinned,
    #[pin]
    stack: [MaybeUninit<u8>; KERNEL_STACK_SIZE],
}

pub struct Metadata {
    /// Is the thread backed by a userspace.
    pub is_usr: bool,
    /// Temporary storage for a thread's stack pointer while it is not running.
    /// Note that this is not accurate for a running thread.
    ///
    /// Used in context switch to switch stack.
    pub(super) rsp: u64,
}
