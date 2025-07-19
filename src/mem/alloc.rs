use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use super::paging::MemoryManager;
use crate::common::log::info;

mod page;
mod slab;

pub use page::PageAllocator;
pub use slab::SlabAllocator;

/// An allocator which fails all allocations and no-op on deallocation.
///
/// This is useful for constructing a `Box` over static memory.
#[derive(Debug, Clone, Copy, Default)]
pub struct StaticAllocator;
unsafe impl Allocator for StaticAllocator {
    fn allocate(&self, _: Layout) -> Result<NonNull<[u8]>, AllocError> { Err(AllocError) }

    /// Performs a no-op.
    ///
    /// # Safety
    /// This function is always _safe_.
    unsafe fn deallocate(&self, _: NonNull<u8>, _: Layout) {}
}

/// The global allocator.
#[derive(Debug, Clone, Copy, Default)]
pub struct GlobalAllocator;
unsafe impl Allocator for GlobalAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        info!(
            "Allocating for size: {:#x}, align: {:#x}",
            layout.size(),
            layout.align()
        );
        let layout = layout.pad_to_align();
        let res = if layout.size() <= SlabAllocator::MAX_SIZE {
            SlabAllocator.allocate(layout)
        } else {
            PageAllocator.allocate(layout)
        };
        info!("Allocated {:?}", res.unwrap());
        res
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        info!(
            "Deallocating for size: {:#x}, align: {:#x}",
            layout.size(),
            layout.align()
        );
        info!("Deallocating {:?}", ptr);
        let layout = layout.pad_to_align();
        if layout.size() <= SlabAllocator::MAX_SIZE {
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
