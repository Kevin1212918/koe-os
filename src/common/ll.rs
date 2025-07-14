use alloc::alloc::Allocator;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::marker::PhantomData;
use core::ptr::NonNull;

use intrusive_collections::{linked_list, Adapter, PointerOps};

/// A node that can be a part of an intrusive linked list.
///
/// # Safety
/// `LINK_OFFSET` should be the byte offset from start of struct to the `Link`
/// field.
///
/// This `Link` field is used to link structures in an intrusive linked list.
pub unsafe trait Linked<const LINK_OFFSET: usize> {
    const OFFSET: usize = LINK_OFFSET;
}

pub type Link = linked_list::Link;

pub trait LinkedPtr<const LINK_OFFSET: usize> {
    type DefaultAdapter: Adapter;
}
pub struct LinkedPtrAdapter<const LINK_OFFSET: usize, P, A>
where
    P: LinkedPtr<LINK_OFFSET>,
    A: Allocator + Clone,
{
    link_ops: linked_list::LinkOps,
    pointer_ops: LinkedPtrOps<LINK_OFFSET, P, A>,
}
unsafe impl<const LINK_OFFSET: usize, P, A> Adapter for LinkedPtrAdapter<LINK_OFFSET, P, A>
where
    P: LinkedPtr<LINK_OFFSET>,
    A: Allocator + Clone,
    LinkedPtrOps<LINK_OFFSET, P, A>: PointerOps,
    <LinkedPtrOps<LINK_OFFSET, P, A> as PointerOps>::Value: Sized,
{
    type LinkOps = linked_list::LinkOps;
    type PointerOps = LinkedPtrOps<LINK_OFFSET, P, A>;

    unsafe fn get_value(
        &self,
        link: <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr,
    ) -> *const <Self::PointerOps as PointerOps>::Value {
        // SAFETY: LINK_OFFSET should be the offset.
        unsafe { link.byte_sub(LINK_OFFSET).as_ptr().cast_const().cast() }
    }
    unsafe fn get_link(
        &self,
        value: *const <Self::PointerOps as PointerOps>::Value,
    ) -> <Self::LinkOps as intrusive_collections::LinkOps>::LinkPtr {
        // SAFETY: LINK_OFFSET should be the offset.
        unsafe { NonNull::new_unchecked(value.byte_add(LINK_OFFSET).cast_mut().cast()) }
    }

    fn link_ops(&self) -> &Self::LinkOps { &self.link_ops }

    fn link_ops_mut(&mut self) -> &mut Self::LinkOps { &mut self.link_ops }

    fn pointer_ops(&self) -> &Self::PointerOps { &self.pointer_ops }
}

pub struct LinkedPtrOps<const LINK_OFFSET: usize, P, A>
where
    P: LinkedPtr<LINK_OFFSET>,
    A: Allocator + Clone,
{
    _phantom: PhantomData<P>,
    alloc: A,
}


pub type LinkedList<const LINK_OFFSET: usize, T: LinkedPtr<LINK_OFFSET>> =
    linked_list::LinkedList<T::DefaultAdapter>;

pub type Cursor<'a, const LINK_OFFSET: usize, T: LinkedPtr<LINK_OFFSET>> =
    linked_list::Cursor<'a, T::DefaultAdapter>;


pub mod boxed {
    use super::*;
    impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone>
        LinkedPtr<LINK_OFFSET> for Box<T, A>
    {
        type DefaultAdapter = LinkedPtrAdapter<LINK_OFFSET, Box<T, A>, A>;
    }
    pub trait BoxLinkedListExt<A: Allocator + Clone> {
        fn new_in(alloc: A) -> Self;
    }
    impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone> BoxLinkedListExt<A>
        for linked_list::LinkedList<LinkedPtrAdapter<LINK_OFFSET, Box<T, A>, A>>
    {
        fn new_in(alloc: A) -> Self {
            let adapter = LinkedPtrAdapter {
                link_ops: linked_list::LinkOps,
                pointer_ops: LinkedPtrOps {
                    alloc,
                    _phantom: PhantomData,
                },
            };
            Self::new(adapter)
        }
    }
    unsafe impl<const LINK_OFFSET: usize, T, A: Allocator + Clone> PointerOps
        for LinkedPtrOps<LINK_OFFSET, Box<T, A>, A>
    where
        T: Linked<LINK_OFFSET>,
        A: Allocator + Clone,
    {
        type Pointer = Box<T, A>;
        type Value = T;

        #[inline]
        unsafe fn from_raw(&self, raw: *const T) -> Box<T, A> {
            unsafe { Box::from_raw_in(raw as *mut T, self.alloc.clone()) }
        }

        #[inline]
        fn into_raw(&self, ptr: Box<T, A>) -> *const T { Box::into_raw(ptr) as *const T }
    }
}

mod arc {
    use super::*;
    impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone>
        LinkedPtr<LINK_OFFSET> for Arc<T, A>
    {
        type DefaultAdapter = LinkedPtrAdapter<LINK_OFFSET, Arc<T, A>, A>;
    }
    unsafe impl<const LINK_OFFSET: usize, T, A> PointerOps for LinkedPtrOps<LINK_OFFSET, Arc<T, A>, A>
    where
        T: Linked<LINK_OFFSET>,
        A: Allocator + Clone,
    {
        type Pointer = Arc<T, A>;
        type Value = T;

        #[inline]
        unsafe fn from_raw(&self, raw: *const T) -> Arc<T, A> {
            unsafe { Arc::from_raw_in(raw as *mut T, self.alloc.clone()) }
        }

        #[inline]
        fn into_raw(&self, ptr: Arc<T, A>) -> *const T { Arc::into_raw(ptr) as *const T }
    }
}
