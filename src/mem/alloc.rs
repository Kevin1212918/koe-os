use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use super::addr::{PageAddr, PageSize};
use super::paging::{Flag, MemoryManager};
use super::phy::PhySpace;
use super::virt::VirtSpace;
use super::UMASpace;

mod page;
mod slab;

pub use page::PageAllocator;
pub use slab::SlabAllocator;

/// The global allocator.
#[derive(Debug, Clone, Copy)]
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

/// Allocates if layout is zero-sized, otherwise returns None.
fn allocate_if_zst(layout: Layout) -> Option<NonNull<[u8]>> {
    if layout.size() != 0 {
        return None;
    }
    Some(NonNull::slice_from_raw_parts(
        NonNull::dangling(),
        0,
    ))
}

/// Deallocates if layout is zero-sized, otherwise returns false.
fn deallocate_if_zst(ptr: NonNull<u8>, layout: Layout) -> bool {
    layout.size() == 0 && ptr == NonNull::dangling()
}

#[global_allocator]
static GLOBAL_ALLOC: GlobalAllocator = GlobalAllocator;
