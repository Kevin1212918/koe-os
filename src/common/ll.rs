use core::cell::{Cell, UnsafeCell};
use core::marker::PhantomPinned;
use core::pin::Pin;
use core::ptr;

use pinned_init::{pin_data, pin_init_from_closure, pinned_drop, PinInit, PinnedDrop};

/// A node that can be a part of an intrusive linked list.
unsafe trait ListNode<const LINK_OFFSET: usize> {}

struct Link(_Link);

#[pin_data(PinnedDrop)]
struct _Link {
    prev: Cell<*mut _Link>,
    next: Cell<*mut _Link>,
    #[pin]
    _pin: PhantomPinned,
}
impl _Link {
    pub fn new() -> impl PinInit<Self, ()> {
        unsafe {
            pin_init_from_closure(|slot: *mut Self| {
                (&raw mut (*slot).prev).write(Cell::new(slot));
                (&raw mut (*slot).next).write(Cell::new(slot));

                Ok(())
            })
        }
    }

    pub fn push_back(mut self: Pin<&mut Self>, mut link: Pin<&mut _Link>) {
        // SAFETY: Converting ref to ptr does not move the referent.
        let link_ptr = ptr::from_mut(unsafe { link.as_mut().get_unchecked_mut() });
        let my_ptr = ptr::from_mut(unsafe { self.as_mut().get_unchecked_mut() });

        link.prev.set(my_ptr);
        link.next.set(self.next.get());

        let next_ptr = self.next.get();
        self.next.set(link_ptr);
    }
}

#[pinned_drop]
impl PinnedDrop for _Link {
    fn drop(self: Pin<&mut Self>) {}
}
/// An intrusive linked list.
struct LinkedList<const LINK_OFFSET: usize> {
    link: _Link,
}
impl<const LINK_OFFSET: usize> LinkedList<LINK_OFFSET> {}
