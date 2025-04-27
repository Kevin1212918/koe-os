use alloc::alloc::Allocator;
use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ptr::NonNull;

use intrusive_collections::{linked_list, Adapter, PointerOps};

/// A node that can be a part of an intrusive linked list.
pub unsafe trait Linked<const LINK_OFFSET: usize> {}

pub type Link = linked_list::Link;

pub trait LinkPointer<const LINK_OFFSET: usize> {
    type DefaultAdapter: Adapter;
}
impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone>
    LinkPointer<LINK_OFFSET> for Box<T, A>
{
    type DefaultAdapter = BoxAdapter<LINK_OFFSET, T, A>;
}

pub type LinkedList<const LINK_OFFSET: usize, T>
where
    T: LinkPointer<LINK_OFFSET>,
= linked_list::LinkedList<T::DefaultAdapter>;

pub trait BoxLinkedListExt<A: Allocator + Clone> {
    fn new_in(alloc: A) -> Self;
}

impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone> BoxLinkedListExt<A>
    for linked_list::LinkedList<BoxAdapter<LINK_OFFSET, T, A>>
{
    fn new_in(alloc: A) -> Self {
        let adapter = BoxAdapter {
            link_ops: linked_list::LinkOps,
            pointer_ops: BoxPointerOps {
                alloc,
                _phantom: PhantomData,
            },
        };
        Self::new(adapter)
    }
}

pub struct BoxAdapter<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone> {
    link_ops: linked_list::LinkOps,
    pointer_ops: BoxPointerOps<T, A>,
}

unsafe impl<const LINK_OFFSET: usize, T: Linked<LINK_OFFSET>, A: Allocator + Clone> Adapter
    for BoxAdapter<LINK_OFFSET, T, A>
{
    type LinkOps = linked_list::LinkOps;
    type PointerOps = BoxPointerOps<T, A>;

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

pub struct BoxPointerOps<T, A: Allocator + Clone> {
    alloc: A,
    _phantom: PhantomData<Box<T, A>>,
}

unsafe impl<T, A: Allocator + Clone> PointerOps for BoxPointerOps<T, A> {
    type Pointer = Box<T, A>;
    type Value = T;

    #[inline]
    unsafe fn from_raw(&self, raw: *const T) -> Box<T, A> {
        unsafe { Box::from_raw_in(raw as *mut T, self.alloc.clone()) }
    }

    #[inline]
    fn into_raw(&self, ptr: Box<T, A>) -> *const T { Box::into_raw(ptr) as *const T }
}
