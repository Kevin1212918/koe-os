use core::cell::{Cell, UnsafeCell};
use core::marker::{PhantomData, PhantomPinned};
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::{self, NonNull};
use core::sync::atomic::AtomicPtr;

/// A node in an intrusive doubly linked list.
///
/// Note despite using [`AtomicPtr`], concurrent operations require external
/// synchronization.
///
/// # Safety
/// Implementor should ensure `Off` is the byte offset from beginning of the
/// struct to its [`link`].
pub unsafe trait ListNode<const Off: usize>: Sized {
    fn link(&self) -> &Link {
        let self_ptr = ptr::from_ref(self);

        // SAFETY: implementation guarentees self + link_offset points to link
        unsafe {
            let link_ptr: *const Link = self_ptr.byte_add(Off).cast();
            link_ptr.as_ref_unchecked()
        }
    }
    /// Append a node after this node..
    fn append(&self, elem: Pin<&Self>) {
        let me = self.link().get();
        let elem = elem.link().get();
        me.append(elem);
    }

    /// Prepend a node before this node..
    fn prepend(&self, elem: Pin<&Self>) {
        let me = self.link().get();
        let elem = elem.link().get();
        me.prepend(elem);
    }

    /// Remove a node from the list.
    ///
    /// # Undefined Behavior
    /// The node should currently be in a list.
    fn remove(&self) {
        let me = self.link().get();
        me.remove();
    }
}

#[repr(transparent)]
pub struct ListHead<const Off: usize, T: ListNode<Off>> {
    list: _Link,
    _data_typ: PhantomData<T>,
}
impl<const Off: usize, T: ListNode<Off>> ListHead<Off, T> {
    pub fn init(mut head: Pin<&mut MaybeUninit<ListHead<Off, T>>>) {
        let list = _Link {
            next: Cell::new(head.as_ptr().cast()),
            prev: Cell::new(head.as_ptr().cast()),
        };
        // list_head will be moved to the correct location after created on
        // the stack
        let list_head = ListHead {
            list,
            _data_typ: PhantomData,
        };
        head.set(MaybeUninit::new(list_head));
    }

    pub fn is_empty(&self) -> bool { self.list.next == self.list.prev }

    pub fn push_back(&self, elem: Pin<&T>) { self.list.prepend(elem.link().get()); }

    pub fn push_front(&self, elem: Pin<&T>) { self.list.append(elem.link().get()); }

    pub fn pop_front(&self) -> Option<*mut T> {
        if self.is_empty() {
            return None;
        }
        let head = self.list.next().expect("self.list is a cirular list");
        head.remove();
        unsafe { Some(head.stru()) }
    }

    pub fn pop_back(&self) -> Option<*mut T> {
        if self.is_empty() {
            return None;
        }
        let tail = self.list.prev().expect("self.list is a cirular list");
        tail.remove();
        unsafe { Some(tail.stru()) }
    }
}

/// The linkage field within an intrusive linked list node.
#[repr(transparent)]
pub struct Link(_Link, PhantomPinned);
impl Link {
    pub fn new() -> Self {
        let link = _Link {
            next: Cell::new(ptr::null()),
            prev: Cell::new(ptr::null()),
        };
        Link(link, PhantomPinned)
    }

    fn get(&self) -> &_Link { &self.0 }
}
struct _Link {
    next: Cell<*const _Link>,
    prev: Cell<*const _Link>,
}

impl _Link {
    /// Calculate reference to a struct from a reference to its link.
    ///
    /// # Safety
    /// `self` should be a field in `T`.
    unsafe fn stru<const Off: usize, T: ListNode<Off>>(&self) -> *mut T {
        let self_ptr = ptr::from_ref(self);

        // SAFETY: implementation guarentees self - link_offset points to
        // the containing struct
        unsafe { self_ptr.byte_sub(Off) as *mut T }
    }

    /// Append a node after this node.
    fn append(&self, elem: &_Link) {
        elem.next.set(self.next.get());
        elem.prev.set(ptr::from_ref(self));

        let elem_ptr = ptr::from_ref(elem);
        // SAFETY: No mutable reference to _Link can be obtained.
        if let Some(next_ref) = unsafe { self.next.get().as_ref() } {
            next_ref.prev.set(elem_ptr);
        }
        self.next.set(elem_ptr);
    }

    /// Prepend a node before this node..
    fn prepend(&self, elem: &_Link) {
        elem.prev.set(self.prev.get());
        elem.next.set(ptr::from_ref(self));

        let elem_ptr = ptr::from_ref(elem);
        // SAFETY: No mutable reference to _Link can be obtained.
        if let Some(prev_ref) = unsafe { self.prev.get().as_ref() } {
            prev_ref.next.set(elem_ptr);
        }
        self.prev.set(elem_ptr);
    }

    /// Remove a node from the list.
    fn remove(&self) {
        let prev_ptr = self.prev.get();
        let next_ptr = self.next.get();

        // SAFETY: No mutable reference to _Link can be obtained.
        if let Some(prev_ref) = unsafe { prev_ptr.as_ref() } {
            prev_ref.next.set(next_ptr);
        }
        // SAFETY: No mutable reference to _Link can be obtained.
        if let Some(next_ref) = unsafe { next_ptr.as_ref() } {
            next_ref.prev.set(prev_ptr);
        }
    }

    /// Get a reference to the next node.
    fn next(&self) -> Option<&Self> { unsafe { self.next.get().as_ref() } }

    /// Get a reference to the previous node.
    fn prev(&self) -> Option<&Self> { unsafe { self.prev.get().as_ref() } }
}
impl Drop for _Link {
    fn drop(&mut self) {
        // Ensures no dropped links are reachable.
        self.remove();
    }
}
