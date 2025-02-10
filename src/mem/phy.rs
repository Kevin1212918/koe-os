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
use crate::mem::kernel_end_lma;

pub(super) fn init(mbi_ptr: usize) { todo!() }

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
    link: Link,
    order: u8,
    flag: Flag,
}
// SAFETY: offset_of! gives the offset of link field in Page
unsafe impl ListNode<{ Page::OFF }> for Page {}

impl Page {
    const OFF: usize = offset_of!(Page, link);

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


type PageList = ListHead<{ Page::OFF }, Page>;

const BUDDY_MAX_ORDER: u8 = 18;
const BUDDY_MAX_DEPTH: u8 = BUDDY_MAX_ORDER;
const BUDDY_MIN_BLOCK_SIZE: usize = PageSize::Small.usize();
const _: () = assert!(BUDDY_MAX_ORDER < u8::MAX);

struct BuddySystem {
    map: ArrayForest<Buddy>,
}
impl BuddySystem {
    /// Create a buddy system that manages `range`.
    ///
    /// # Undefined Behavior
    /// `range` should be aligned to [`BUDDY_MIN_BLOCK_SIZE`]
    ///
    /// TODO: check range in bound.
    ///
    /// # Panic
    /// See [`BitForest::new`] for `buf` requirements.
    pub fn new(pmm: &PhysicalMemoryManager, alloc: impl Allocator) -> Result<Self, AllocError> {
        let page_cnt = pmm.frames.len();
        let dummy_page_cnt = page_cnt.next_power_of_two();

        let max_order = (dummy_page_cnt.ilog2() as u8).max(BUDDY_MAX_ORDER);

        let tree_depth = max_order + 1;
        let tree_cnt = page_cnt.div_ceil(1 << max_order);
        let mut map = ArrayForest::new(
            tree_cnt,
            tree_depth as usize,
            alloc,
            Buddy::free(0),
        )?;
        for d in 0..tree_depth {
            // Initialize dummy pages as reserved.


            let order = max_order - d;
            let fill_cnt = page_cnt >> order;
            let partial_order = (page_cnt - (fill_cnt << order))
                .checked_ilog2()
                .unwrap_or(0) as u8;

            assert!(partial_order <= order);
            let fill = Buddy::free(order);
            let partial = Buddy::free(partial_order);
            let empty = Buddy::reserved();

            let dslice = map.slice_mut(d as usize);
            dslice[0..fill_cnt].fill(fill);
            dslice[fill_cnt] = partial;
            dslice[fill_cnt + 1..].fill(empty);
        }

        let buddy = BuddySystem { map };

        Ok(buddy)
    }

    /// Free a reserved page.
    ///
    /// # Undefined Behavior
    /// `pfn` should be previously reserved through this [`BuddySystem`]
    pub fn free(&mut self, pmm: &mut PhysicalMemoryManager, pfn: Pfn) {
        let page = pmm.page(pfn).expect("pfn should be valid.");
        let depth = Self::order2depth(&self, page.order);
        let idx = Self::pfn2idx(pfn, page.order);

        let mut cursor = self.map.cursor_mut(depth, idx);

        let buddy = cursor.get_mut();
        assert!(
            buddy.is_free(),
            "BuddySystem: Double Free!"
        );
        *buddy = Buddy::free(page.order);

        Self::fixup_map(&mut cursor);
    }

    /// Reserve a page.
    fn reserve(&mut self, pmm: &mut PhysicalMemoryManager, order: u8) -> Option<Pfn> {
        let idx = self.reserve_on_map(order)?;

        // Mark Page struct
        let pfn = Self::idx2pfn(pmm, idx, order);
        let page = pmm.page_mut(pfn).expect("idx2pfn should return valid pfn.");
        *page.order() = order;

        Some(pfn)
    }

    /// Reserve a page on map. Returns the index of the reserved buddy on map.
    fn reserve_on_map(&mut self, order: u8) -> Option<usize> {
        let mut cursor_opt = None;
        let mut stack: ArrayVec<_, { BUDDY_MAX_DEPTH as usize }> = ArrayVec::new();

        for (idx, root) in self.map.slice_mut(0).iter_mut().enumerate() {
            if root.is_free() && root.order() > order {
                cursor_opt = Some(self.map.cursor(0, idx));
                break;
            }
        }

        // No root contains a block with target order.
        if cursor_opt.is_none() {
            return None;
        }

        while let Some(ref mut cursor) = cursor_opt {
            if cursor.get().is_reserved() {
                cursor_opt = stack.pop();
                continue;
            }

            let cur_max_order = self.depth2order(cursor.depth());
            let cur_order = cursor.get().order();

            if cur_order < order {
                // Rewind
                cursor_opt = stack.pop();
            } else if cur_order == order && cur_order == cur_max_order {
                // Found block
                break;
            } else {
                // Keep searching
                let mut rewind = cursor.clone();
                let not_end = rewind.right();
                assert!(not_end);
                stack.push(rewind);
                cursor.left();
            }
        }

        // Allow reborrowing later
        drop(stack);

        let Some(cursor) = cursor_opt else {
            unreachable!("A properly sized block should be found under the root.");
        };
        // Found block
        let idx = cursor.idx();

        // Reborrow cursor as mut cursor
        let target_depth = cursor.depth();
        let target_idx = cursor.idx();
        let mut cursor = self.map.cursor_mut(target_depth, target_idx);

        // Update map
        *cursor.get_mut() = Buddy::reserved();
        Self::fixup_map(&mut cursor);

        Some(idx)
    }

    fn pfn2idx(pfn: Pfn, order: u8) -> usize { pfn.0.get() >> order }

    fn idx2pfn(pmm: &PhysicalMemoryManager, idx: usize, order: u8) -> Pfn {
        pmm.pfn_from_raw(idx << order).unwrap()
    }

    const fn depth2order(&self, depth: usize) -> u8 { (self.map.max_depth() - depth) as u8 }

    const fn order2depth(&self, order: u8) -> usize { self.map.max_depth() - order as usize }

    fn fixup_map(cursor: &mut Cursor<&mut ArrayForest<Buddy>, Buddy>) {
        while cursor.depth() != 0 {
            let me = *cursor.get();
            cursor.sibling();
            let sib = *cursor.get();

            let par = Buddy::max(me, sib);
            cursor.up();

            // No need to fix further
            if *cursor.get_mut() == par {
                return;
            }
            *cursor.get_mut() = par;
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Buddy(u8);
impl Buddy {
    /// Creates a free buddy.
    ///
    /// # Undefined Behavior
    /// `order` < `u8::MAX`
    const fn free(order: u8) -> Self {
        debug_assert!(order != u8::MAX);
        Buddy(order + 1)
    }

    const fn reserved() -> Self { Buddy(0) }

    const fn is_free(self) -> bool { !self.is_reserved() }

    const fn is_reserved(self) -> bool { self.0 == 0 }

    /// Returns a reference to order of a free buddy. Returns u8::MAX if buddy
    /// is not free.
    const fn order(&self) -> u8 { self.0 - 1 }
}



// const BUDDY_MAX_ORDER: usize = 18;
// const BUDDY_MAX_DEPTH: usize = BUDDY_MAX_ORDER;
// const BUDDY_MIN_BLOCK_SIZE: usize = PageSize::Small.usize();

// //* TODO: Fix addr representation to allow generic addr range */
// pub struct BuddySystem {
//     max_order: u8,
//     free_lists: [PageList; BUDDY_MAX_ORDER],
//     _pin: PhantomPinned,
// }


// impl BuddySystem {

//     /// Create a buddy system that manages `range`.
//     ///
//     /// # Undefined Behavior
//     /// `range` should be aligned to [`BUDDY_MIN_BLOCK_SIZE`]
//     ///
//     /// TODO: check range in bound.
//     ///
//     /// # Panic
//     /// See [`BitForest::new`] for `buf` requirements.
//     pub fn new(range: Range<usize>, alloc: impl Allocator)
//         -> Result<Pin<&'static mut Self>, AllocError>
//     {
//         let page_cnt = (range.end - range.start) / BUDDY_MIN_BLOCK_SIZE;
//         let max_order = page_cnt.next_power_of_two().trailing_zeros() as
// usize;

//         // NOTE: we are leaking tbi_ptr here.
//         let tbi_ptr: *mut Self =
// alloc.allocate(Layout::new::<Self>())?.cast().as_ptr();

//         unsafe { (&raw mut (*tbi_ptr).max_order).write(max_order as u8); }
//         for i in 0..BUDDY_MAX_ORDER {
//             let tbi_list_ptr = unsafe { (&raw mut (*tbi_ptr).free_lists[i])
// };             let tbi_list = unsafe {
// tbi_list_ptr.cast::<MaybeUninit<PageList>>().as_mut_unchecked() };
//             let tbi_list = unsafe { Pin::new_unchecked(tbi_list) };
//             PageList::init(tbi_list);
//         }

//         let tbi = unsafe { tbi_ptr.as_mut_unchecked() };
//         Ok(unsafe { Pin::new_unchecked(tbi) })
//     }

//     /// Release the frame pointed by `pfn`.
//     pub fn release(&mut self, pmm: &mut PhysicalMemoryManager, pfn: Pfn) {
//         let mut tbi_opt = Some(pfn);
//         while let Some(tbi) = tbi_opt {
//             tbi_opt = self.release_at_order(pmm, tbi);
//         }
//     }

//     /// Release the frame pointed by `pfn`. Returns a [`Pfn`] if the page
//     /// is merged and need to be released at a higher order.
//     fn release_at_order(&mut self, pmm: &mut PhysicalMemoryManager, pfn: Pfn)
// -> Option<Pfn> {

//         fn insert_into_free_list(
//             list: &mut PageList,
//             pmm: &mut PhysicalMemoryManager,
//             pfn: Pfn
//         ) {
//             let mut page = pmm.page_mut(pfn).unwrap();
//             page.as_mut().flag().insert(Flag::IN_BUDDY);
//             list.push_front(page.as_ref());
//         }

//         let Some(page) = pmm.page(pfn) else { return None; };
//         let list = &mut self.free_lists[page.order as usize];
//         if list.is_empty() {
//             insert_into_free_list(list, pmm, pfn);
//             return None;
//         };
//         let Some(buddy) = Self::buddy(pmm, &page) else {
//             insert_into_free_list(list, pmm, pfn);
//             return None;
//         };
//         if !buddy.flag.contains(Flag::IN_BUDDY) {
//             insert_into_free_list(list, pmm, pfn);
//             return None;
//         }

//         // Merge buddy

//         // SAFETY: Pin<&Page> have same layout as *const Page, and the
//         // resulting pointers are not being dereferenced.
//         let page_ptr: *const Page = unsafe { mem::transmute_copy(&page) };
//         let buddy_ptr: *const Page = unsafe { mem::transmute_copy(&buddy) };

//         buddy.remove();
//         let buddy_pfn = pmm.pfn(&buddy);

//         if page_ptr < buddy_ptr {
//             let buddy = pmm.page_mut(buddy_pfn).unwrap();
//             buddy.flag().remove(Flag::IN_BUDDY);

//             let page = pmm.page_mut(pfn).unwrap();
//             *page.order() += 1;
//             return Some(pfn);
//         } else {
//             let buddy = pmm.page_mut(buddy_pfn).unwrap();
//             *buddy.order() += 1;
//             return Some(buddy_pfn);
//         }
//     }



//     fn buddy<'a, 'b> (pmm: &'b PhysicalMemoryManager, page: &'a Page) ->
// Option<Pin<&'b Page>> {         let my_order = page.order;
//         let buddy_mask = 1 << my_order;
//         let my_pfn = pmm.pfn(page);

//         let buddy_pfn = my_pfn.0.get() ^ buddy_mask;
//         let buddy_pfn = pmm.pfn_from_raw(buddy_pfn)?;

//         Some(pmm.page(buddy_pfn).expect("return value of pfn_from_raw should
// be valid"))     }
// }



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

        let mut mge_min: Option<Addr<LinearSpace>> = None;
        let mut mge_max: Option<Addr<LinearSpace>> = None;

        for (is_free, range) in page_ranges {
            mge_min = mge_min
                .map(|x| x.min(range.start()))
                .or(Some(range.start()));

            mge_max = mge_max.map(|x| x.max(range.end())).or(Some(range.end()));

            if is_free {
                free.push(range.clone());
            } else {
                reserved.push(range.clone());
            }
        }

        let (Some(mge_min), Some(mge_max)) = (mge_min, mge_max) else {
            panic!("should not invoke BootMemoryManager::new with no memory");
        };

        free.sort_unstable_by_key(|x| x.start());
        reserved.sort_unstable_by_key(|x| x.start());

        let managed_range = (mge_min..mge_max).contained_pages(Self::PAGE_SIZE);
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

impl PageManager<LinearSpace> for BootMemoryManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageRange<LinearSpace>> {
        let mut record = self.0.lock();

        for i in 0..record.free.len() {
            let block = unsafe { record.free.get_unchecked_mut(i) };
            let contained_pages = block.range().contained_pages(page_size);
            if contained_pages.len() < cnt {
                continue;
            }

            let start = PageAddr::new(contained_pages.start(), page_size);
            let target = PageRange::new(start, cnt);

            let residuals = block.range().range_sub(target.range()).map(|x| {
                PageRange::try_from_range(x, BootMemoryRecord::PAGE_SIZE)
                    .expect("residual ranges should be aligned to the smallest page size")
            });

            let residual_state = (
                residuals[0].is_empty(),
                residuals[1].is_empty(),
            );
            match residual_state {
                (false, false) => {
                    record.free.remove(i);
                },
                (true, false) => {
                    record.free[i] = residuals[0];
                },
                (false, true) => {
                    record.free[i] = residuals[1];
                },
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
        let pages = PageRange::try_from_range(
            pages.range(),
            BootMemoryRecord::PAGE_SIZE,
        )
        .expect("all page ranges should be aligned to the smallest page size");

        let idx = record
            .free
            .binary_search_by_key(&pages.start(), |x| x.start())
            .expect_err("cannot deallocate a free'd page");

        let merge_front = (idx > 0) && record.free[idx - 1].end() == pages.start();
        let merge_back = (idx < record.free.len()) && record.free[idx].start() == pages.end();

        match (merge_front, merge_back) {
            (false, false) => {
                record.free.insert(idx, pages);
            },
            (false, true) => {
                record.free[idx].len += pages.len();
            },
            (true, false) => {
                record.free[idx - 1].len += pages.len();
            },
            (true, true) => {
                record.free[idx - 1].len += pages.len();
                record.free[idx - 1].len += record.free[idx].len();
                record.free.remove(idx);
            },
        };
    }
}
