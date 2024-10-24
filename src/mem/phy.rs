use core::{alloc::Layout, cell::{OnceCell, SyncUnsafeCell}, hash::BuildHasherDefault, iter, marker::PhantomPinned, mem, ops::{Add, DerefMut}, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, slice, usize};

use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};
use spin::Mutex;

use crate::{boot, mem::{addr::Pages, kernel_end_lma, kernel_size, kernel_start_lma, phy_to_virt}};

use super::addr::{AddrRange as _, PAddr, PPage, PPages, PRange, PageBitmap, PageSize, VAddr};

pub(super) static BIT_ALLOCATOR: spin::Mutex<Option<BitmapAllocator>> = spin::Mutex::new(None);

pub(super) fn init(mbi_ptr: usize) {
    todo!()
}

/// An page aligned allocator for physical memory
pub trait Allocator {
    /// Allocates contiguous `cnt` of `page_size`-sized pages
    /// 
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PPage>;

    /// Allocates contiguous `cnt` of `page_size`-sized pages which starts 
    /// at `at`. If the `cnt` pages starting at `at` is not available to 
    /// allocate, this tries to allocate some other contiguous pages.
    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: PPage) -> Option<PPage>;

    /// Deallocate `page`
    /// 
    /// # Safety
    /// `page` should be a page allocated by this allocator.
    unsafe fn deallocate_pages(&self, page: PPage, cnt: usize);
}

/// Find the initial free memory areas.
/// 
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'boot>(
    boot_info: &'boot BootInformation
) -> impl Iterator<Item = PRange> + 'boot{
    let mbi_range = unsafe {
        let start = PAddr::from_usize(boot_info.start_address());
        let end = PAddr::from_usize(boot_info.end_address());
        start .. end
    };
    let memory_areas = boot_info.memory_map_tag()
        .expect("BootInformation should include memory map").memory_areas();

    let available: MemoryAreaTypeId = multiboot2::MemoryAreaType::Available.into();
    let kernel_area = kernel_start_lma() .. kernel_end_lma();
    memory_areas
        .iter()
        .filter(move |area| 
            area.typ() == available
        )
        .map(|area| unsafe {
            let start = PAddr::from_usize(area.start_address() as usize);
            let end = PAddr::from_usize(area.end_address() as usize);
            start .. end
        })
        .flat_map(move |range| range.range_sub(kernel_area.clone()))
        .filter(|x|!x.is_empty())
        .flat_map(move |range| range.range_sub(mbi_range.clone()))
        .filter(|x|!x.is_empty())
}

/// Find the initial range of available physical memory
fn initial_memory_range(boot_info: &BootInformation) -> PRange {
    let memory_areas = boot_info.memory_map_tag()
        .expect("BootInformation should include memory map").memory_areas();

    let (mut min, mut max) = (usize::MAX, 0);
    for area in memory_areas {
        min = usize::min(area.start_address() as usize, min);
        max = usize::max(area.end_address() as usize, max);
    }
    assert!(min < max, "BootInformation memory map should not be empty");

    unsafe {
        PAddr::from_usize(min) .. PAddr::from_usize(max+1)
    }
}

type PPageBitmap = PageBitmap<{BitmapAllocator::PAGE_SIZE.into_usize()}, PAddr>;

/// Page allocator backed by a bitmap over the managed pages.
/// 
/// Note that this allocator only supports the page size `Self::PAGE SIZE`, 
/// and will panic when called with any other page sizes.
pub(super) struct BitmapAllocator {
    bitmap: spin::Mutex<&'static mut PPageBitmap>
}
impl BitmapAllocator {
    const PAGE_SIZE: PageSize = PageSize::Small;

    fn new(boot_info: &BootInformation) -> Self {
        let init_mem_pages = initial_memory_range(boot_info)
            .contained_pages(Self::PAGE_SIZE);
        
        let bitmap_size = PPageBitmap::bytes_required(
            init_mem_pages.len()
        );

        let bitmap_pages = initial_free_memory_areas(boot_info)
            .map(|free_area| free_area.contained_pages(Self::PAGE_SIZE))
            .find(|free_pages| {
                free_pages.len() >= bitmap_size
            });
        let Some(bitmap_pages) = bitmap_pages else {
            panic!("System should have enough memory to hold a PageBitmap");
        };

        let bitmap_addr = phy_to_virt(bitmap_pages.start());
        let bitmap_ref = unsafe { 
            PageBitmap::init(
                bitmap_addr.into_ptr(), 
                init_mem_pages.start(), 
                init_mem_pages.len()
            ) 
        };
        unsafe { 
            bitmap_ref.set_unchecked(
                bitmap_pages.start(), 
                bitmap_pages.len(), 
                true
            ) 
        };

        Self {bitmap: spin::Mutex::new(bitmap_ref)}
    }
}
impl Allocator for BitmapAllocator {
    fn allocate_pages(&self, size: usize, page_size: PageSize) -> Option<PPage> {
        assert!(page_size == Self::PAGE_SIZE);
        let page_byte_size = page_size.into_usize();

        let page_cnt = size.div_ceil(page_byte_size);
        let base = self.bitmap.lock().find_unoccupied(page_cnt)?;
        unsafe { self.bitmap.lock().set_unchecked(base, page_cnt, true) };

        Some(PPage::new(base, page_size))
    }
    
    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: PPage) -> Option<PPage> {
        todo!()
    }

    unsafe fn deallocate_pages(&self, page: PPage, cnt: usize) {
        let size = cnt * Self::PAGE_SIZE.into_usize();
        self.bitmap.lock().set(page.start(), size, false);
    }
}

impl Allocator for boot::MemblockAllocator<'_> {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PPage> {
        let layout = Layout::from_size_align(
            cnt * page_size.into_usize(), 
            page_size.alignment()
        ).expect("page_size should convert to a valid layout");
        self.allocate(layout).map(|addr| PPage::new(addr, page_size))
    }

    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: PPage) -> Option<PPage> {
        let layout = Layout::from_size_align(
            cnt * page_size.into_usize(), 
            page_size.alignment()
        ).expect("page_size should convert to a valid layout");
        self.allocate_at(layout, at.start().into_usize())
            .map(|addr| PPage::new(addr, page_size))
    }

    unsafe fn deallocate_pages(&self, page: PPage, _cnt: usize) {
        unsafe {self.deallocate(page.start())};
    }
}