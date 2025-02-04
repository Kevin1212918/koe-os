use core::{alloc::Layout, cell::{OnceCell, SyncUnsafeCell}, hash::BuildHasherDefault, iter, marker::{PhantomData, PhantomPinned}, mem, ops::{Add, DerefMut, Range}, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, slice, usize};

use arrayvec::ArrayVec;
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};

use crate::{boot, common::{ll, TiB}, mem::kernel_end_lma};

use super::{addr::{Addr, AddrRange as _, AddrSpace, PageAddr, PageManager, PageRange, PageSize,}, alloc::allocate_pages, kernel_start_lma, p2v, paging::{MemoryManager, X86_64MemoryManager}, virt::{BumpMemoryManager, VAllocSpace}};

pub(super) fn init(mbi_ptr: usize) {
    todo!()
}

pub trait PhySpace: AddrSpace {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinearSpace;
impl PhySpace for LinearSpace {}
impl AddrSpace for LinearSpace {
    const RANGE: Range<usize> = {
        let start = 0;
        let end = 64 * TiB;
        start .. end
    };
}

type PAddr = Addr<LinearSpace>;

/// Find the initial free memory areas.
/// 
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'boot>(
    boot_info: &'boot BootInformation
) -> impl Iterator<Item = Range<PAddr>> + 'boot{
    let mbi_range = unsafe {
        let start = PAddr::new(boot_info.start_address());
        let end = PAddr::new(boot_info.end_address());
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
            let start = PAddr::new(area.start_address() as usize);
            let end = PAddr::new(area.end_address() as usize);
            start .. end
        })
        .flat_map(move |range| range.range_sub(kernel_area.clone()))
        .filter(|x|!x.is_empty())
        .flat_map(move |range| range.range_sub(mbi_range.clone()))
        .filter(|x|!x.is_empty())
}

/// Find the initial range of available physical memory
fn initial_memory_range(boot_info: &BootInformation) -> Range<PAddr> {
    let memory_areas = boot_info.memory_map_tag()
        .expect("BootInformation should include memory map").memory_areas();

    let (mut min, mut max) = (usize::MAX, 0);
    for area in memory_areas {
        min = usize::min(area.start_address() as usize, min);
        max = usize::max(area.end_address() as usize, max);
    }
    assert!(min < max, "BootInformation memory map should not be empty");

    unsafe {
        PAddr::new(min) .. PAddr::new(max+1)
    }
}



struct Page {
    link: ll::Link<Self>,
    flag: u8,
}

struct PhysicalPageManager {
    pages: NonNull<[Page]>,
}
impl ll::LinkNode for Page {
    fn link(&self) -> &ll::Link<Self> { &self.link }
}
impl PhysicalPageManager {
    const BUDDY_MAX_DEPTH: u8;

    fn new(
        mmu: &X86_64MemoryManager,
        managed_range: PageRange<LinearSpace>,
        free_ranges: impl Iterator<Item = PageRange<LinearSpace>>,
    ) -> Self {
        let buf_page_size = PageSize::Small;
        let buf_page_cnt = ( range.len() * size_of::<Page>() ).div_ceil(buf_page_size.usize());

        todo!()
    }
}

impl PageManager<LinearSpace> for PhysicalPageManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageRange<LinearSpace>> {
        assert!(cnt == 1, "StackPageManager supports only one page");
        todo!()
    }

    unsafe fn deallocate_pages(&self, pages: PageRange<LinearSpace>) {
        assert!(pages.len() == 1, "StackPageManager supports only one page");
        todo!()
    }
}













/// Static physical memory manager. This is used to initialize and is replaced 
/// by [PhysicalPageManager]
struct BootMemoryRecord {
    reserved: ArrayVec<PageRange<LinearSpace>, { Self::MAX_FRAGMENT }>,
    free: ArrayVec<PageRange<LinearSpace>, { Self::MAX_FRAGMENT }>,
    managed_range: PageRange<LinearSpace>,
}
impl BootMemoryRecord {
    const MAX_FRAGMENT: usize = 128;
    const PAGE_SIZE: PageSize = PageSize::Small;

    /// Creates a new [`BootMemoryManager`].
    /// 
    /// The argument `page_ranges` specifies consists of tuples 
    /// `(is_free, range)`, where `is_free` specifies if the range is 
    /// initially free. 
    fn new<'a>(page_ranges: impl Iterator<Item = (bool, &'a PageRange<LinearSpace>)>) -> Self {
        let mut reserved = ArrayVec::new();
        let mut free = ArrayVec::new();

        let mut mge_min: Option<Addr<LinearSpace>> = None;
        let mut mge_max: Option<Addr<LinearSpace>> = None;

        for (is_free, range) in page_ranges {
            mge_min = mge_min
                .map(|x| x.min(range.start()))
                .or(Some(range.start()));

            mge_max = mge_max
                .map(|x| x.max(range.end()))
                .or(Some(range.end()));

            if is_free {
                free.push(range.clone());
            } else {
                reserved.push(range.clone());
            }
        }

        let (Some(mge_min), Some(mge_max)) = (mge_min, mge_max) else {
            panic!("should not invoke BootMemoryManager::new with no memory");
        };

        free.sort_unstable_by_key(|x|x.start());
        reserved.sort_unstable_by_key(|x|x.start());

        let managed_range = (mge_min .. mge_max).contained_pages(Self::PAGE_SIZE);
        Self { reserved, free, managed_range } 
    }
}

struct BootMemoryManager(spin::Mutex<BootMemoryRecord>);
impl BootMemoryManager {
    fn new<'a>(page_ranges: impl Iterator<Item = (bool, &'a PageRange<LinearSpace>)>) -> Self {
        Self (spin::Mutex::new(BootMemoryRecord::new(page_ranges)))
    }
}

impl PageManager<LinearSpace> for BootMemoryManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageRange<LinearSpace>> {
        let mut record = self.0.lock();

        for i in 0 .. record.free.len() {
            let block = unsafe { record.free.get_unchecked_mut(i) };
            let contained_pages = block.range().contained_pages(page_size);
            if contained_pages.len() < cnt { continue; }

            let start = PageAddr::new(contained_pages.start(), page_size);
            let target = PageRange::new(start, cnt);

            let residuals = block.range().range_sub(target.range())
                .map(|x| PageRange::try_from_range(x, BootMemoryRecord::PAGE_SIZE)
                .expect("residual ranges should be aligned to the smallest page size"));

            match (residuals[0].is_empty(), residuals[1].is_empty()) {
                (false, false) => { record.free.remove(i); },
                (true, false) => { record.free[i] = residuals[0]; },
                (false, true) => { record.free[i] = residuals[1]; },
                (true, true) => { 
                    record.free[i] = residuals[1];
                    record.free.insert(i, residuals[0]); 
                },
            }

            return Some(target);
        }

        return None;
    }

    unsafe fn deallocate_pages(&self, pages: PageRange<LinearSpace>) {
        let mut record = self.0.lock();
        let pages = PageRange::try_from_range(pages.range(), BootMemoryRecord::PAGE_SIZE)
            .expect("all page ranges should be aligned to the smallest page size");

        let idx = record.free.binary_search_by_key(&pages.start(), |x|x.start())
            .expect_err("cannot deallocate a free'd page");

        let merge_front = (idx > 0) && record.free[idx-1].end() == pages.start();
        let merge_back = (idx < record.free.len()) && record.free[idx].start() == pages.end();

        match (merge_front, merge_back) {
            (false, false) => {
                record.free.insert(idx, pages);
            },
            (false, true) => {
                record.free[idx].len += pages.len();
            },
            (true, false) => {
                record.free[idx-1].len += pages.len();
            },
            (true, true) => {
                record.free[idx-1].len += pages.len();
                record.free[idx-1].len += record.free[idx].len();
                record.free.remove(idx);
            },
        };
    }
}
