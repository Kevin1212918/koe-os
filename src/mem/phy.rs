use core::{alloc::Layout, cell::{OnceCell, SyncUnsafeCell}, hash::BuildHasherDefault, iter, marker::PhantomPinned, mem, ops::{Add, DerefMut}, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, slice, usize};

use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};

use crate::mem::{addr::Pages, kernel_end_lma, kernel_size, kernel_start_lma, phy_to_virt};

use super::addr::{AddrRange as _, PAddr, PPage, PPages, PRange, PageBitmap, PageSize, VAddr};

pub(super) static BIT_ALLOCATOR: spin::Mutex<Option<BitmapAllocator>> = spin::Mutex::new(None);

pub(super) fn init(mbi_ptr: usize) {

    BIT_ALLOCATOR.lock().get_or_insert_with(|| BitmapAllocator::new(boot_info));
}

pub(super) trait Allocator {
    /// Allocate a page of size `page_size`.
    /// 
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_page(&self, page_size: PageSize) -> Option<PPage>;
    /// Allocates contiguous `page_size`-sized pages whose sizes summed up is 
    /// at least of size `size`.
    /// 
    /// Note that each of the allocated pages should be deallocated 
    /// individually.
    /// 
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_contiguous(&self, size: usize, page_size: PageSize) -> Option<PPages>;
    /// Deallocate `page`
    /// 
    /// # Safety
    /// `page` should be a page allocated by this allocator.
    unsafe fn deallocate(&self, page: PPage);
}

/// Find the initial free memory areas.
/// 
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'bootloader>(
    boot_info: &'bootloader BootInformation
) -> impl Iterator<Item = PRange> + 'bootloader{
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
    fn allocate_page(&self, page_size: PageSize) -> Option<PPage> {
        assert!(page_size == Self::PAGE_SIZE);

        let base = self.bitmap.lock().find_unoccupied(1)?;
        unsafe { self.bitmap.lock().set_unchecked(base, 1, true) };
        Some(PPage::new(base, page_size))
    }

    fn allocate_contiguous(&self, size: usize, page_size: PageSize) -> Option<PPages> {
        assert!(page_size == Self::PAGE_SIZE);
        let page_byte_size = page_size.into_usize();

        let page_cnt = size.div_ceil(page_byte_size);
        let base = self.bitmap.lock().find_unoccupied(page_cnt)?;
        unsafe { self.bitmap.lock().set_unchecked(base, page_cnt, true) };

        let start_page = PPage::new(base, page_size);
        let end_page = PPage::new(base.byte_add(page_cnt * page_byte_size), page_size);
        Some(Pages::new(start_page, end_page))
    }

    unsafe fn deallocate(&self, page: PPage) {
        self.bitmap.lock().set(page.start(), Self::PAGE_SIZE.into_usize(), false);
    }
}

/// An extent of memory used in `MemblockAllocator``. Note present `Memblock` 
/// is considered greater than not present `Memblock`
#[derive(Debug, Default, Clone, Copy)]
struct Memblock {
    is_present: usize,
    base: usize,
    size: usize,
}
impl PartialEq for Memblock {
    fn eq(&self, other: &Self) -> bool {
        self.is_present == other.is_present && 
        (self.is_present == 0 || self.base == other.base)
    }
}
impl Eq for Memblock {}
impl PartialOrd for Memblock {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match self.is_present.partial_cmp(&other.is_present) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord.map(|x|x.reverse()),
        }
        if self.is_present == 0 { return Some(core::cmp::Ordering::Equal); }
        self.base.partial_cmp(&other.base)
    }
}
impl Ord for Memblock {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).expect("Memblock order should be total")
    }
}
impl Memblock {
    fn is_empty(&self) -> bool { self.is_present == 0 }
    const fn empty() -> Self { Self { is_present: 0, base: 0, size: 0 } }
    const fn new(base: usize, size: usize) -> Self { 
        Self {is_present: 1, base, size }
    }
}

/// A reference to a sorted array of `Memblock`s.
struct Memblocks<'boot> {
    data: &'boot mut [Memblock], 
    len: usize,
}
impl<'boot> Memblocks<'boot> {
    fn new(blocks: &'boot mut [Memblock]) -> Self {
        blocks.sort_unstable();
        Self {data: blocks, len: 0}
    }
    fn len(&self) -> usize {
        self.len
    }
    fn put_block(&mut self, block: Memblock) -> bool {
        // Memblocks is full
        if self.len == self.data.len() { return false; }
        // Inserting empty block is no-op
        if block.is_empty() { return true; }


        let Some(pivot) = self.data.binary_search(&block).err() else {
            // Inserting overlapping block
            return false;
        };

        // Check if block overlap or merge with prev or next block
        let mut merge_prev: Option<usize> = None;
        let mut merge_next: Option<usize> = None;

        if pivot > 0 {
            // SAFETY: Given pivot > 0, pivot - 1 should be in bound
            let prev = unsafe {self.data.get_unchecked(pivot - 1)};
            debug_assert!(!prev.is_empty());
            debug_assert!(prev.base <= block.base);
            if block.base - prev.base < prev.size { return false; }
            if block.base - prev.base == prev.size { merge_prev = Some(pivot - 1)};
        }

        // SAFETY: pivot should be in bound
        let next = unsafe {self.data.get_unchecked(pivot)};
        debug_assert!(next.base >= block.base);
        if !next.is_empty() {
            if next.base - block.base < block.size { return false; }
            if next.base - block.base == block.size { merge_next = Some(pivot); }
        }

        match (merge_prev, merge_next) {
            (None, None) => {
                self.data[pivot ..= self.len].rotate_right(1);
                self.data[pivot] = block;
                self.len += 1;
            },
            (None, Some(idx)) => {
                self.data[idx].base = block.base;
                self.data[idx].size += block.size;
            },
            (Some(idx), None) => {
                self.data[idx].size += block.size;
            },
            (Some(prev), Some(pivot)) => {
                debug_assert!(pivot != self.len);
                self.data[prev].size += self.data[pivot].size + block.size;
                self.data[pivot] = Memblock::empty();
                self.data[pivot .. self.len].rotate_left(1);
                self.len -= 1;
            },
        }

        true
    }
    fn cut_block(&mut self, layout: Layout) -> Option<Memblock> {
        let (cut_idx, cut_base) = self.data.iter().enumerate()
        .find_map(|(idx, b)| {
            if b.is_empty() { return None }
            let unaligned_base = b.base + (b.size.checked_sub(layout.size())?);
            let aligned_base = unaligned_base - unaligned_base % layout.align();
            (aligned_base >= b.base).then_some((idx, aligned_base))
        })?;
        let base = cut_base;
        let size = self.data[cut_idx].size - (cut_base - self.data[cut_idx].base);

        self.data[cut_idx].size = cut_base - self.data[cut_idx].base;
        if self.data[cut_idx].size == 0 {
            self.data[cut_idx] = Memblock::empty();
            self.data[cut_idx .. self.len].rotate_left(1);
            self.len -= 1;
        }

        Some(Memblock::new(base, size))
    }
    fn take_block(&mut self, base: usize) -> Option<Memblock> {
        let idx = self.data
        .binary_search_by_key(&base, |b| {
            if b.is_empty() { usize::MAX } else { b.base }
        }).ok()?;

        let ret = self.data[idx];
        self.data[idx] = Memblock::empty();
        self.data[idx .. self.len].rotate_left(1);
        self.len -= 1;

        Some(ret)
    }
} 

pub struct MemblockAllocator<'boot> {
    free_blocks: Memblocks<'boot>,
    reserved_blocks: Memblocks<'boot>,
}
impl MemblockAllocator<'_> {
    const MAX_MEMBLOCKS_LEN: usize = 128;
    fn init<'boot>() -> MemblockAllocator<'boot> {
        static FREE_BLOCKS: SyncUnsafeCell<[Memblock; MemblockAllocator::MAX_MEMBLOCKS_LEN]> = 
            SyncUnsafeCell::new([Memblock::empty(); MemblockAllocator::MAX_MEMBLOCKS_LEN]);
        static RESERVED_BLOCKS: SyncUnsafeCell<[Memblock; MemblockAllocator::MAX_MEMBLOCKS_LEN]> = 
            SyncUnsafeCell::new([Memblock::empty(); MemblockAllocator::MAX_MEMBLOCKS_LEN]);

        let free_blocks: Memblocks<'_>;
        let reserved_blocks: Memblocks<'_>;

        unsafe {
            free_blocks = Memblocks::new(
                FREE_BLOCKS.get().as_mut_unchecked().as_mut_slice()); 
            reserved_blocks = Memblocks::new(
                RESERVED_BLOCKS.get().as_mut_unchecked().as_mut_slice());
        }

        MemblockAllocator { free_blocks, reserved_blocks }
    }
    fn add_managed_range(&mut self, base: usize, size: usize) -> bool {
        self.free_blocks.put_block()
    }
}

