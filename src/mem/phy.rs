use alloc::alloc::{AllocError, Allocator};
use alloc::collections::binary_heap::BinaryHeap;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::{Cell, OnceCell, SyncUnsafeCell};
use core::hash::BuildHasherDefault;
use core::marker::{PhantomData, PhantomPinned};
use core::mem::{self, offset_of, MaybeUninit};
use core::ops::{Add, DerefMut, Range};
use core::pin::Pin;
use core::ptr::{self, slice_from_raw_parts_mut, NonNull};
use core::{iter, slice, usize};

use arrayvec::ArrayVec;
use buddy::BuddySystem;
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};
use nonmax::{NonMaxU8, NonMaxUsize};

use super::addr::{Addr, AddrRange as _, AddrSpace, PageAddr, PageManager, PageRange, PageSize};
use super::alloc::allocate_pages;
use super::paging::{MemoryManager, X86_64MemoryManager};
use super::{kernel_start_lma, p2v};
use crate::boot;
use crate::common::array_forest::{self, ArrayForest, Cursor};
use crate::common::ll::{self, Link, ListHead, ListNode};
use crate::common::TiB;
use crate::mem::addr::AddrRange;
use crate::mem::kernel_end_lma;

pub(super) fn init(mbi_ptr: usize) { todo!() }

mod buddy;
mod memblock;

pub trait PhySpace: AddrSpace {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LinearSpace;
impl PhySpace for LinearSpace {}
impl AddrSpace for LinearSpace {
    const RANGE: Range<usize> = {
        let start = 0;
        let end = 64 * TiB;
        start..end
    };
}

type PAddr = Addr<LinearSpace>;

/// Find the initial free memory areas.
///
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'boot>(
    boot_info: &'boot BootInformation,
) -> impl Iterator<Item = Range<PAddr>> + 'boot {
    let mbi_range = unsafe {
        let start = PAddr::new(boot_info.start_address());
        let end = PAddr::new(boot_info.end_address());
        start..end
    };
    let memory_areas = boot_info
        .memory_map_tag()
        .expect("BootInformation should include memory map")
        .memory_areas();

    let available: MemoryAreaTypeId = multiboot2::MemoryAreaType::Available.into();
    let kernel_area = kernel_start_lma()..kernel_end_lma();
    memory_areas
        .iter()
        .filter(move |area| area.typ() == available)
        .map(|area| unsafe {
            let start = PAddr::new(area.start_address() as usize);
            let end = PAddr::new(area.end_address() as usize);
            start..end
        })
        .flat_map(move |range| range.range_sub(kernel_area.clone()))
        .filter(|x| !x.is_empty())
        .flat_map(move |range| range.range_sub(mbi_range.clone()))
        .filter(|x| !x.is_empty())
}

/// Find the initial range of available physical memory
fn initial_memory_range(boot_info: &BootInformation) -> Range<PAddr> {
    let memory_areas = boot_info
        .memory_map_tag()
        .expect("BootInformation should include memory map")
        .memory_areas();

    let (mut min, mut max) = (usize::MAX, 0);
    for area in memory_areas {
        min = usize::min(area.start_address() as usize, min);
        max = usize::max(area.end_address() as usize, max);
    }
    assert!(
        min < max,
        "BootInformation memory map should not be empty"
    );

    unsafe { PAddr::new(min)..PAddr::new(max + 1) }
}

bitflags::bitflags! {
struct Flag: u8 {
}}

struct Page {
    order: u8,
    flag: Flag,
}

impl Page {
    fn order(self: Pin<&mut Self>) -> &mut u8 {
        // SAFETY: order field is not pinned.
        &mut unsafe { self.get_unchecked_mut() }.order
    }

    fn flag(self: Pin<&mut Self>) -> &mut Flag {
        // SAFETY: order field is not pinned.
        &mut unsafe { self.get_unchecked_mut() }.flag
    }
}


// static PMM: spin::Once<PhysicalMemoryManager> = spin::Once::new();

#[derive(Debug, Clone, Copy)]
pub struct Pfn(NonMaxUsize);

struct PhysicalMemoryManager {
    frames: &'static mut [Page],
    base: PageAddr<LinearSpace>,
    buddy: BuddySystem,
}
impl PhysicalMemoryManager {
    fn page(&self, pfn: Pfn) -> Option<Pin<&Page>> {
        // SAFETY: All pages are pinned
        self.frames
            .get(pfn.0.get())
            .map(|x| unsafe { Pin::new_unchecked(x) })
    }

    fn page_mut(&mut self, pfn: Pfn) -> Option<Pin<&mut Page>> {
        // SAFETY: All pages are pinned
        self.frames
            .get_mut(pfn.0.get())
            .map(|x| unsafe { Pin::new_unchecked(x) })
    }

    const fn pfn_from_raw(&self, pfn: usize) -> Option<Pfn> {
        if pfn >= self.frames.len() {
            return None;
        }

        // SAFETY: pfn is less than self.frames.len(), which is less than max.
        Some(Pfn(unsafe {
            NonMaxUsize::new_unchecked(pfn)
        }))
    }

    const fn pfn(&self, page: &Page) -> Pfn {
        let page_ptr = ptr::from_ref(page);
        // SAFETY: Page structs are all located within PMM.frames
        let idx: isize = unsafe { page_ptr.offset_from(self.frames_ptr()) };
        let idx: usize = idx as usize;
        // SAFETY: idx cannot be too large
        Pfn(unsafe { NonMaxUsize::new_unchecked(idx) })
    }

    const fn frames_ptr(&self) -> *const Page { &raw const self.frames[0] }
}


impl PhysicalMemoryManager {
    fn new(
        mmu: &impl MemoryManager,
        managed_range: PageRange<LinearSpace>,
        free_ranges: impl Iterator<Item = PageRange<LinearSpace>>,
    ) -> Self {
        let buf_page_size = PageSize::Small;
        todo!()
    }
}

impl PageManager<LinearSpace> for PhysicalMemoryManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageRange<LinearSpace>> {
        assert!(
            cnt == 1,
            "StackPageManager supports only one page"
        );
        todo!()
    }

    unsafe fn deallocate_pages(&self, pages: PageRange<LinearSpace>) {
        assert!(
            pages.len() == 1,
            "StackPageManager supports only one page"
        );
        todo!()
    }
}

// ------------ arch ----------------



/// Static physical memory manager. This is used to initialize and is replaced
/// by [`PhysicalPageManager`]
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

        let mut mge_min = LinearSpace::RANGE.end;
        let mut mge_max = LinearSpace::RANGE.start;

        for (is_free, range) in page_ranges {
            mge_min = usize::min(mge_min, range.start().usize());
            mge_max = usize::max(mge_max, range.end().usize());

            if is_free {
                free.push(*range);
            } else {
                reserved.push(*range);
            }
        }

        assert!(
            !(mge_min..mge_max).is_empty(),
            "page_ranges should not be empty"
        );


        let managed_range =
            (Addr::new(mge_min)..Addr::new(mge_max)).contained_pages(Self::PAGE_SIZE);
        let frames_layout = Layout::array::<Page>(managed_range.len())
            .expect("page frames array should not be too large");

        let frames_range_idx = free
            .iter()
            .enumerate()
            .find(|(_, f)| f.size() < frames_layout.size())
            .map(|pair| pair.0)
            .expect("MemoryRecord: Not enough memory for frames.");

        let frames_page_cnt = frames_layout
            .size()
            .next_multiple_of(Self::PAGE_SIZE.usize());
        let frames_page_range = PageRange::new(
            PageAddr::new(
                free[frames_range_idx].start(),
                Self::PAGE_SIZE,
            ),
            frames_page_cnt,
        );



        let residual_page_cnt = free[frames_range_idx].len() - frames_page_cnt;
        if residual_page_cnt != 0 {
            let residual_page_range = PageRange::new(
                PageAddr::new(frames_page_range.end(), Self::PAGE_SIZE),
                residual_page_cnt,
            );
            free.push(residual_page_range);
        };

        free.swap_remove(frames_range_idx);
        reserved.push(frames_page_range);



        Self {
            reserved,
            free,
            managed_range,
        }
    }
}

struct BootMemoryManager(spin::Mutex<BootMemoryRecord>);
impl BootMemoryManager {
    fn new<'a>(page_ranges: impl Iterator<Item = (bool, &'a PageRange<LinearSpace>)>) -> Self {
        Self(spin::Mutex::new(BootMemoryRecord::new(
            page_ranges,
        )))
    }
}
