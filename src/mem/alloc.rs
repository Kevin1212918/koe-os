use alloc::boxed::Box;
use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::iter::empty;
use core::marker::PhantomData;
use core::mem::{offset_of, MaybeUninit};
use core::ops::Div;
use core::ptr::{self, slice_from_raw_parts, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU8, AtomicUsize};

use bitvec::order::Lsb0;
use bitvec::slice::BitSlice;
use bitvec::view::BitView;
use intrusive_collections::{
    intrusive_adapter, DefaultPointerOps, LinkedList, PointerOps, UnsafeRef,
};
use page::PageAllocator;
use slab::{Cache, SlabAllocator, UntypedCache};

use super::addr::{Addr, PageManager, PageSize};
use super::paging::{Flag, MemoryManager};
use super::phy::PhysicalMemoryManager;
use super::virt::VirtSpace;
use super::LinearSpace;
use crate::mem::addr::{AddrRange, AddrSpace, PageRange};
use crate::mem::paging::MMU;
use crate::mem::phy::PMM;
use crate::mem::virt::PhysicalRemapSpace;

mod page;
mod slab;

fn allocate_pages<V: VirtSpace>(
    mmu: &impl MemoryManager,
    vmm: &mut impl PageManager<V>,
    pmm: &mut impl PageManager<LinearSpace>,

    cnt: usize,
    page_size: PageSize,
) -> Result<NonNull<[u8]>, AllocError> {
    let vbase = vmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;
    let pbase = pmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;

    let ptr = NonNull::new(vbase.base.addr().into_ptr())
        .expect("successfull virtual page allocation should not return null address");

    debug_assert!(vbase.len == cnt);
    debug_assert!(pbase.len == cnt);

    let flags = [Flag::Present, Flag::ReadWrite];

    for (vpage, ppage) in Iterator::zip(vbase.into_iter(), pbase.into_iter()) {
        unsafe {
            mmu.map(vpage, ppage, flags, pmm).expect("TODO: cleanup");
        }
    }

    Ok(NonNull::slice_from_raw_parts(
        ptr,
        page_size.usize(),
    ))
}

#[global_allocator]
static GLOBAL_ALLOC: GlobalAllocator = GlobalAllocator;
pub struct GlobalAllocator;
unsafe impl Allocator for GlobalAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if layout.pad_to_align().size() <= SlabAllocator::MAX_SIZE {
            SlabAllocator.allocate(layout)
        } else {
            PageAllocator.allocate(layout)
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.pad_to_align().size() <= SlabAllocator::MAX_SIZE {
            unsafe { SlabAllocator.deallocate(ptr, layout) }
        } else {
            unsafe { PageAllocator.deallocate(ptr, layout) }
        }
    }
}
unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout)
            .map(|ptr| ptr.cast::<u8>().as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(ptr) = NonNull::new(ptr) else {
            return;
        };
        unsafe { self.deallocate(ptr, layout) }
    }
}

fn allocate_if_zst(layout: Layout) -> Option<NonNull<[u8]>> {
    if layout.size() != 0 {
        return None;
    }
    Some(NonNull::slice_from_raw_parts(
        NonNull::dangling(),
        0,
    ))
}

fn deallocate_if_zst(ptr: NonNull<u8>, layout: Layout) -> bool {
    layout.size() == 0 && ptr == NonNull::dangling()
}
