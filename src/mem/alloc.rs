
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
//         if layout.size() == 0 || layout.align() > Self::PAGE_SIZE.into_usize() { 
//             return Err(AllocError)
//         }
//         let new_allocate_size = layout.size();
//         let mut inner = self.inner.lock();

//         let residual_page_size_opt = inner.cur_offset.checked_next_multiple_of(layout.align())
//             .and_then(|cur_aligned_off| Self::PAGE_SIZE.into_usize().checked_sub(cur_aligned_off))
//             .filter(|&residual_page_size| new_allocate_size < residual_page_size);


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
//                     .expect("Successful palloc allocation should not return no pages");
//                 inner.cur_offset = res_end.addr_sub(inner.cur_page.start()) as usize;
//                 res
//             }
//         };

//         let res_ptr = unsafe { NonNull::new_unchecked(phy_to_virt(res_paddr).into_ptr::<u8>()) };
//         Ok(NonNull::slice_from_raw_parts(res_ptr, new_allocate_size))
//     }
// }
// struct BootAllocatorInner {
//     cur_page: PPage,
//     cur_offset: usize
// }

// unsafe impl Allocator for BootAllocator {
//     fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
//         self.allocate(layout)
//     }

//     unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
//         debug_assert!(false, "BootAllocator cannot deallocate memory");
//     }
// }