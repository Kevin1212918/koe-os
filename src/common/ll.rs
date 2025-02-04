use core::{pin::Pin, sync::atomic::AtomicPtr};

/// A node in an intrusive doubly linked list. 
/// 
/// Note despite using [`AtomicPtr`], concurrent operations require external
/// synchronization.
pub trait LinkNode: Sized {
    fn link(&self) -> &Link<Self>;

    fn append(&self, elem: Pin<&Self>) {
        use core::sync::atomic::Ordering;
        let elem_link = elem.link();
        let my_link = self.link();

        let my_next = my_link.next.load(Ordering::Relaxed);
        elem_link.next.store(my_next, Ordering::Relaxed);
        let me = core::ptr::from_ref(self).cast_mut();
        elem_link.prev.store(me, Ordering::Relaxed);

        let elem_ptr = core::ptr::from_ref(elem.get_ref()).cast_mut();
        if let Some(next_ref) = unsafe { my_link.next.load(Ordering::Relaxed).as_ref() } {
            next_ref.link().prev.store(elem_ptr, Ordering::Relaxed);
        }
        my_link.next.store(elem_ptr, Ordering::Relaxed);
    }
    fn prepend(&self, elem: Pin<&Self>) {
        use core::sync::atomic::Ordering;
        let elem_link = elem.link();
        let my_link = self.link();

        let my_prev = my_link.prev.load(Ordering::Relaxed);
        elem_link.prev.store(my_prev, Ordering::Relaxed);
        let me = core::ptr::from_ref(self).cast_mut();
        elem_link.next.store(me, Ordering::Relaxed);

        let elem_ptr = core::ptr::from_ref(elem.get_ref()).cast_mut();
        if let Some(prev_ref) = unsafe { my_link.prev.load(Ordering::Relaxed).as_ref() } {
            prev_ref.link().next.store(elem_ptr, Ordering::Relaxed);
        }
        my_link.prev.store(elem_ptr, Ordering::Relaxed);
    }
    fn remove(&self) {
        use core::sync::atomic::Ordering;
        let my_link = self.link();
        let prev_ptr = my_link.prev.load(Ordering::Relaxed);
        let next_ptr = my_link.next.load(Ordering::Relaxed);

        if let Some(prev_ref) = unsafe { prev_ptr.as_ref() } {
            prev_ref.link().next.store(next_ptr, Ordering::Relaxed);
        }
        if let Some(next_ref) = unsafe { next_ptr.as_ref() } {
            next_ref.link().prev.store(prev_ptr, Ordering::Relaxed);
        }
    }
    fn next(&self) -> Option<Pin<&Self>> {
        use core::sync::atomic::Ordering;
        let unpinned = unsafe { self.link().next.load(Ordering::Relaxed).as_ref() };
        unpinned.map(|x| unsafe { Pin::new_unchecked(x) })
    }
    fn prev(&self) -> Option<Pin<&Self>> {
        use core::sync::atomic::Ordering;
        let unpinned = unsafe { self.link().prev.load(Ordering::Relaxed).as_ref() };
        unpinned.map(|x| unsafe { Pin::new_unchecked(x) })
    }
}

pub struct Link<T> {
    next: AtomicPtr<T>,
    prev: AtomicPtr<T>,
}