use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::fmt::Write as _;
use core::ops::Div as _;
use core::ptr::NonNull;

use crate::common::hlt;
use crate::drivers::vga::VGA_BUFFER;
use crate::log;
use crate::mem::addr::{Addr, AddrRange, AddrSpace, PageManager, PageRange, PageSize};
use crate::mem::alloc::{allocate_if_zst, deallocate_if_zst};
use crate::mem::paging::{Flag, MemoryManager, MMU};
use crate::mem::phy::PMM;
use crate::mem::virt::PhysicalRemapSpace;

// TODO: Auto huge page
#[derive(Debug, Clone, Copy)]
pub struct PageAllocator;
unsafe impl Allocator for PageAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some(ptr) = allocate_if_zst(layout) {
            return Ok(ptr);
        }

        debug_assert!(PageSize::MIN.align() % layout.align() == 0);
        let page_cnt = layout
            .size()
            .checked_next_multiple_of(PageSize::MIN.usize())
            .ok_or(AllocError)?
            .div(PageSize::MIN.usize());
        let page_size = PageSize::MIN;

        let mut pmm = PMM.get().ok_or(AllocError)?.lock();

        let prange = pmm.allocate_pages(page_cnt, page_size).ok_or(AllocError)?;
        debug_assert!(prange.len >= page_cnt);
        debug_assert!(prange.page_size() >= page_size);

        let vbase = PhysicalRemapSpace::p2v(prange.base.addr());
        // let vrange = AddrRange::new(vbase, page_cnt * page_size.usize());
        // let vrange = PageRange::try_from_range(vrange, page_size)
        //     .expect("vbase and size should be page_aligned.");

        let ptr = NonNull::new(vbase.into_ptr())
            .expect("successfull virtual page allocation should not return null address");

        // let flags = [Flag::Present, Flag::ReadWrite];
        // for (vpage, ppage) in Iterator::zip(vrange.into_iter(), prange.into_iter()) {
        // unsafe {
        // MMU.map(vpage, ppage, flags, &mut pmm)
        // .expect("TODO: cleanup");
        // }
        // }

        Ok(NonNull::slice_from_raw_parts(
            ptr,
            page_size.usize() * prange.len,
        ))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if deallocate_if_zst(ptr, layout) {
            return;
        }

        let page_cnt = layout
            .size()
            .next_multiple_of(PageSize::MIN.usize())
            .div(PageSize::MIN.usize());
        let page_size = PageSize::MIN;

        let vbase = ptr.as_ptr() as usize;
        assert!(
            PhysicalRemapSpace::RANGE.contains(&vbase),
            "Try deallocating unallocated memory"
        );
        let vbase: Addr<PhysicalRemapSpace> = Addr::new(ptr.as_ptr() as usize);
        let pbase = PhysicalRemapSpace::v2p(vbase);
        let prange = AddrRange::new(pbase, page_cnt * page_size.usize());
        let prange = PageRange::try_from_range(prange, page_size)
            .expect("pbase and size should be page_aligned.");

        let mut pmm = PMM.get().expect("PMM should have been initialized").lock();
        unsafe { pmm.deallocate_pages(prange) };
    }
}
