// First fit allocator using only the bitmap physical frame allocator, and
// returning virtual memory address in the physical remap space.
//
// `BootAllocator` can only be used for allocation of less than a small
// page, and should switch to another allocaor once `mem` module is fully
// initialized.

// pub(super) struct BootAllocator {
//     inner: spin::Mutex<BootAllocatorInner>,
// }
// impl BootAllocator {
//     const PAGE_SIZE: PageSize = PageSize::Small;

//     fn new() -> Self {
//         let cur_page = phy::BIT_ALLOCATOR.lock().as_mut()
//             .expect("phy::BitmapAllocator should exist")
//             .allocate_page(Self::PAGE_SIZE)
//             .expect("BootAllocator should initialize successfully");
//         let cur_offset = 0;
//         let inner = spin::Mutex::new(
//             BootAllocatorInner {cur_page, cur_offset}
//         );
//         Self {inner}
//     }

//     fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//         if layout.size() == 0 || layout.align() >
// Self::PAGE_SIZE.into_usize() {             return Err(AllocError)
//         }
//         let new_allocate_size = layout.size();
//         let mut inner = self.inner.lock();

//         let residual_page_size_opt =
// inner.cur_offset.checked_next_multiple_of(layout.align())
// .and_then(|cur_aligned_off|
// Self::PAGE_SIZE.into_usize().checked_sub(cur_aligned_off))
// .filter(|&residual_page_size| new_allocate_size < residual_page_size);


//         let res_paddr = match residual_page_size_opt {
//             Some(_) => {
//                 let res = inner.cur_page.start().byte_add(inner.cur_offset);
//                 inner.cur_offset += new_allocate_size;
//                 res
//             },
//             None => {

//                 let new_pages = phy::BIT_ALLOCATOR.lock().as_mut()
//                     .expect("phy::BitmapAllocator should exist")
//                     .allocate_contiguous(
//                         new_allocate_size,
//                         Self::PAGE_SIZE
//                     ).ok_or(AllocError)?;
//                 let res = new_pages.start();
//                 let res_end = new_pages.start().byte_add(new_allocate_size);
//                 inner.cur_page = new_pages.into_iter().last()
//                     .expect("Successful palloc allocation should not return
// no pages");                 inner.cur_offset =
// res_end.addr_sub(inner.cur_page.start()) as usize;                 res
//             }
//         };

//         let res_ptr = unsafe {
// NonNull::new_unchecked(phy_to_virt(res_paddr).into_ptr::<u8>()) };
//         Ok(NonNull::slice_from_raw_parts(res_ptr, new_allocate_size))
//     }
// }

use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::ptr::NonNull;

use super::addr::{PageManager, PageSize};
use super::paging::{Flag, MemoryManager};
use super::virt::VirtSpace;
use super::LinearSpace;

pub fn allocate_pages<V: VirtSpace>(
    mmu: &impl MemoryManager,
    vmm: &impl PageManager<V>,
    pmm: &impl PageManager<LinearSpace>,

    cnt: usize,
    page_size: PageSize,
) -> Result<NonNull<[u8]>, AllocError> {
    let vbase = vmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;
    let pbase = pmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;

    let ptr = NonNull::new(vbase.start().into_ptr())
        .expect("successfull virtual page allocation should not return null address");

    debug_assert!(vbase.len() == cnt);
    debug_assert!(pbase.len() == cnt);

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
static DUMMY_ALLOC: DummyAllocator = DummyAllocator;
struct DummyAllocator;
unsafe impl GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unimplemented!()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unimplemented!()
    }
}