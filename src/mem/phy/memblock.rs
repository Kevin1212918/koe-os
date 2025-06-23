use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;
use core::iter::{self, Peekable};
use core::mem::MaybeUninit;
use core::ops::Deref;

use arrayvec::ArrayVec;
use derive_more::derive::IntoIterator;
use multiboot2::{BootInformation, MemoryArea, MemoryAreaType};

use crate::mem::addr::{Addr, AddrRange, AddrSpace, PageAddr, PageRange, PageSize};
use crate::mem::{kernel_end_lma, kernel_start_lma, UMASpace};

pub fn init(boot_info: &BootInformation) -> &'static mut MemblockSystem {
    // SAFETY: BMM is not accessed elsewhere in the module, and init is called
    // only once.
    let bmm = unsafe { BMM.get().as_mut_unchecked() };
    let mb = MemblockSystem::init(bmm);

    // Initialize available memorys
    let memory_areas = boot_info
        .memory_map_tag()
        .expect("Multiboot should provide memory map tag")
        .memory_areas();
    for area in memory_areas {
        if let MemoryAreaType::Available = area.typ().into() {
            let start = Addr::new(area.start_address() as usize);
            let end = Addr::new(area.end_address() as usize);
            mb.add(AddrRange::from(start..end));
        }
    }

    // Mark the first physical page as reserved.
    let start = UMASpace::MIN_ADDR;
    let end = start + PageSize::MIN.usize();
    mb.reserve(AddrRange::from(start..end));

    // Mark the kernel region as reserved.
    let start = kernel_start_lma();
    let end = kernel_end_lma();
    mb.reserve(AddrRange::from(start..end));

    // Mark boot info as reserved.
    let start = Addr::new(boot_info.start_address() as usize);
    let end = Addr::new(boot_info.end_address() as usize);
    mb.reserve(AddrRange::from(start..end));

    // Mark all boot modules as reserved.
    for module in boot_info.module_tags() {
        let start = Addr::new(module.start_address() as usize);
        let end = Addr::new(module.end_address() as usize);
        mb.reserve(AddrRange::from(start..end));
    }

    mb
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum MemTyp {
//     Free,
//     Reserved,
// }

///// An extent of memory used in `BootMemoryManager``. Note present `Memblock`
///// is considered greater than not present `Memblock`
//#[derive(Debug, Clone, Copy)]
//pub struct Memblock {
//    pub range: AddrRange<UMASpace>,
//    pub typ: MemTyp,
//}
//impl Memblock {
//    /// Returns an iterator of power-of-2 aligned memblocks, whose order is
//    /// in between `min_order` and `max_order`, inclusive.
//    pub fn aligned_split(mut self, min_order: u8, max_order: u8) ->
// AlignedSplit {        'success: {
//            let min_align = 1 << min_order;
//
//            let Some(base) = self.range.start().align_ceil(min_align) else {
//                break 'success;
//            };
//
//            self.range.base = base;
//
//            let Some(end) = self.range.end().align_floor(min_align) else {
//                break 'success;
//            };
//
//            self.range.size = match (end - base).try_into() {
//                Ok(x) => x,
//                Err(_) => break 'success,
//            };
//
//            return AlignedSplit {
//                memblock: self,
//                offset: 0,
//                max_order: max_order as u32,
//            };
//        }
//
//        // Returning an empty iterator.
//        AlignedSplit {
//            memblock: self,
//            offset: self.range.size,
//            max_order: max_order as u32,
//        }
//    }
//
//    pub fn order(&self) -> u8 { self.range.start().usize().trailing_zeros() as
// u8 }
//}
//impl PartialEq for Memblock {
//    fn eq(&self, other: &Self) -> bool { self.range.start() ==
// other.range.start() }
//}
//impl Eq for Memblock {}
//impl PartialOrd for Memblock {
//    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
//        self.range.start().partial_cmp(&other.range.start())
//    }
//}
//impl Ord for Memblock {
//    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
//        self.range.start().cmp(&other.range.start())
//    }
//}
//
//impl From<&MemoryArea> for Memblock {
//    fn from(value: &MemoryArea) -> Self {
//        let ma_typ: MemoryAreaType = value.typ().into();
//        let typ = match ma_typ {
//            MemoryAreaType::Available => MemTyp::Free,
//            MemoryAreaType::Reserved
//            | MemoryAreaType::AcpiAvailable
//            | MemoryAreaType::ReservedHibernate
//            | MemoryAreaType::Defective
//            | MemoryAreaType::Custom(_) => MemTyp::Reserved,
//        };
//        Memblock {
//            range: AddrRange {
//                base: Addr::new(value.start_address() as usize),
//                size: value.size() as usize,
//            },
//            typ,
//        }
//    }
//}

const MEMBLOCKS_LEN: usize = 128;

/// A set union of `AddrRange`s.
pub struct Memblocks {
    data: ArrayVec<AddrRange<UMASpace>, MEMBLOCKS_LEN>,
    bound: AddrRange<UMASpace>,
}
impl Memblocks {
    fn new() -> Self {
        Memblocks {
            data: ArrayVec::new(),
            bound: AddrRange::empty(),
        }
    }

    /// Merge a range into this `Memblocks`. Returns true
    /// if merge occurs.
    fn insert(&mut self, block: AddrRange<UMASpace>) -> bool {
        let merge_start = self
            .data
            .iter()
            .position(|r| r.start() >= block.start())
            .unwrap_or(self.data.len());
        let mut merge_cnt = 0;

        let mut merged_range = block;

        for cur in self.data[merge_start..].iter() {
            match merged_range.range_sum_strict(cur) {
                Some(mr) => {
                    merge_cnt += 1;
                    merged_range = mr;
                },
                None => break,
            }
        }

        self.data.drain(merge_start..merge_start + merge_cnt);
        self.data.insert(merge_start, merged_range);

        self.update_bound();
        merge_cnt != 0
    }

    fn update_bound(&mut self) {
        debug_assert!(self.data.iter().map(|x| x.start()).is_sorted());
        if self.data.is_empty() {
            self.bound = AddrRange::empty();
            return;
        }
        let bound_start = self.data.first().unwrap().start();
        let bound_end = self.data.last().unwrap().end();
        self.bound = AddrRange::from(bound_start..bound_end);
    }
}

static BMM: SyncUnsafeCell<MaybeUninit<MemblockSystem>> =
    SyncUnsafeCell::new(MaybeUninit::uninit());

// TODO: Implement freeing allocation and memory regions in Memblock.
pub struct MemblockSystem {
    memory_blocks: Memblocks,
    reserved_blocks: Memblocks,
    is_frozen: bool,
}
impl MemblockSystem {
    /// In-place initialize a `MemblockSystem`.
    pub fn init(slot: &mut MaybeUninit<MemblockSystem>) -> &mut MemblockSystem {
        let tbi = slot.as_mut_ptr();
        // SAFETY: Initializing free_blocks
        unsafe { (&raw mut ((*tbi).memory_blocks)).write(Memblocks::new()) };
        // SAFETY: Initializing reserved_blocks
        unsafe { (&raw mut ((*tbi).reserved_blocks)).write(Memblocks::new()) };

        // SAFETY: slot.map_unchecked_mut returns reference to an union variant
        // of the pinned value. tbi is initialized from above.
        unsafe { slot.assume_init_mut() }
    }

    /// Add the `region` to be managed.
    ///
    /// The `region` will be available for reservation.
    pub fn add(&mut self, region: AddrRange<UMASpace>) {
        if !region.is_empty() {
            self.memory_blocks.insert(region);
        }
    }

    /// Returns an iterator to the ranges available for reservation.
    pub fn available_regions(&self) -> impl Iterator<Item = AddrRange<UMASpace>> + '_ {
        let memory_regions = self.memory_blocks.data.iter().cloned().peekable();

        // Get the negated range by taking ranges from previous end to next start.
        let reserved_range_start = self.reserved_blocks.data.iter().map(|x| x.end());
        let reserved_range_start = iter::once(UMASpace::MIN_ADDR).chain(reserved_range_start);

        let reserved_range_end = self.reserved_blocks.data.iter().map(|x| x.start());
        let reserved_range_end = reserved_range_end.chain(iter::once(UMASpace::MAX_ADDR));

        let not_reserved_regions = reserved_range_start
            .zip(reserved_range_end)
            .map(|(start, end)| AddrRange::from(start..end))
            .peekable();

        IntersectRanges {
            ranges1: memory_regions,
            ranges2: not_reserved_regions,
        }
    }

    /// Mark the `region` as reserved. Reserving an empty range will be a no-op.
    ///
    /// # Undefined Behavior
    /// The region should *available*. In other words, the region should be
    /// fully contained by a range returned from
    /// [`available_regions`](Self::available_regions).
    pub fn reserve(&mut self, region: AddrRange<UMASpace>) {
        if !region.is_empty() {
            debug_assert!(self
                .reserved_blocks
                .data
                .iter()
                .all(|x| !x.overlaps(&region)));

            self.reserved_blocks.insert(region);
        }
    }

    /// Return the bounding range on currently managed memory regions.
    pub fn managed_range(&self) -> AddrRange<UMASpace> { self.memory_blocks.bound }

    /// Returns an iterator to the ranges reserved.
    ///
    /// # Note
    /// Each range does not necessarily correspond to one reservation. Multiple
    /// reservations may span one range.
    pub fn reserved_regions(&self) -> impl Iterator<Item = AddrRange<UMASpace>> + '_ {
        self.reserved_blocks.data.iter().cloned()
    }

    /// Returns an iterator to the memory ranges.
    pub fn memory_regions(&self) -> impl Iterator<Item = AddrRange<UMASpace>> + '_ {
        self.memory_blocks.data.iter().cloned()
    }
}

pub struct IntersectRanges<I1, I2>
where
    I1: Iterator<Item = AddrRange<UMASpace>>,
    I2: Iterator<Item = AddrRange<UMASpace>>,
{
    ranges1: Peekable<I1>,
    ranges2: Peekable<I2>,
}

impl<I1, I2> Iterator for IntersectRanges<I1, I2>
where
    I1: Iterator<Item = AddrRange<UMASpace>>,
    I2: Iterator<Item = AddrRange<UMASpace>>,
{
    type Item = AddrRange<UMASpace>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut r1_opt = self.ranges1.peek();
        let mut r2_opt = self.ranges2.peek();

        loop {
            let r1 = r1_opt.cloned()?;
            let r2 = r2_opt.cloned()?;

            let intersect = r1.range_intersect(&r2);

            if r1.end() <= r2.end() {
                self.ranges1.next();
                r1_opt = self.ranges1.peek();
            }
            if r2.end() <= r1.end() {
                self.ranges2.next();
                r2_opt = self.ranges2.peek();
            }

            if !intersect.is_empty() {
                return Some(intersect);
            }
        }
    }
}
