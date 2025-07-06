use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::ops::Div as _;
use core::ptr::NonNull;

use crate::mem::addr::{self, Addr, AddrRange, AddrSpace, PageRange, PageSize};
use crate::mem::alloc::{allocate_if_zst, deallocate_if_zst};
use crate::mem::phy::PhysicalMemoryManager;
use crate::mem::virt::PhysicalRemapSpace;
use crate::mem::UMASpace;

// TODO: Auto huge page

/// A page allocator that will only allocate to the nearest page bound.
///
/// For now, this will only allocate [`PageSize::MIN`] page.
#[derive(Debug, Clone, Copy)]
pub struct PageAllocator;
unsafe impl addr::Allocator<UMASpace> for PageAllocator {
    fn allocate(&self, layout: Layout) -> Option<AddrRange<UMASpace>> {
        if layout.size() == 0 {
            return Some(AddrRange::empty());
        }

        let layout = layout.pad_to_align();
        let page_cnt = layout
            .size()
            .next_multiple_of(PageSize::MIN.usize())
            .div(PageSize::MIN.usize());
        let page_size = PageSize::MIN;

        let prange = PhysicalMemoryManager.allocate_pages(page_cnt, page_size)?;
        debug_assert!(prange.len >= page_cnt);
        debug_assert!(prange.page_size() >= page_size);

        Some(prange.into())
    }

    unsafe fn deallocate(&self, addr: Addr<UMASpace>, layout: Layout) {
        if layout.size() == 0 {
            return;
        }

        let page_cnt = layout
            .size()
            .next_multiple_of(PageSize::MIN.usize())
            .div(PageSize::MIN.usize());
        let page_size = PageSize::MIN;

        let prange = AddrRange::new(addr, page_cnt * page_size.usize());
        let prange = PageRange::try_from_range(prange, page_size)
            .expect("pbase and size should be page_aligned.");

        unsafe { PhysicalMemoryManager.deallocate_pages(prange) };
    }
}

unsafe impl Allocator for PageAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some(ptr) = allocate_if_zst(layout) {
            return Ok(ptr);
        }

        let phy = <Self as addr::Allocator<UMASpace>>::allocate(self, layout).ok_or(AllocError)?;
        let base = unsafe { NonNull::new_unchecked(PhysicalRemapSpace::p2v(phy.base).into_ptr()) };
        Ok(NonNull::slice_from_raw_parts(
            base, phy.size,
        ))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        assert!(
            PhysicalRemapSpace::RANGE.contains(&(ptr.as_ptr() as usize)),
            "Try deallocating unallocated memory"
        );
        unsafe {
            <Self as addr::Allocator<UMASpace>>::deallocate(
                self,
                PhysicalRemapSpace::v2p(Addr::from_mut_ptr(ptr.as_ptr())),
                layout,
            )
        };
    }
}


/// A frame allocator that will only allocate to the nearest page bound.
///
/// For now, this will only allocate [`PageSize::MIN`] page.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalPageManager;
unsafe impl Allocator for PhysicalPageManager {
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

        let prange = PhysicalMemoryManager
            .allocate_pages(page_cnt, page_size)
            .ok_or(AllocError)?;
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

        unsafe { PhysicalMemoryManager.deallocate_pages(prange) };
    }
}
