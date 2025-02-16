use alloc::alloc::Allocator;
use core::alloc::Layout;
use core::ops::{Deref, Range};
use core::pin::Pin;
use core::ptr::{self, slice_from_raw_parts_mut, NonNull};
use core::usize;

use arrayvec::ArrayVec;
use buddy::{BuddySystem, BUDDY_MAX_ORDER};
use memblock::{MemblockAllocator, MemblockSystem, Memblocks};
use multiboot2::{BootInformation, MemoryArea, MemoryAreaTypeId};
use nonmax::NonMaxUsize;

use super::addr::{Addr, AddrRange as _, AddrSpace, PageAddr, PageManager, PageRange, PageSize};
use super::kernel_start_lma;
use super::paging::{MemoryManager, MMU};
use super::virt::PhysicalRemapSpace;
use crate::common::ll::ListNode;
use crate::common::TiB;
use crate::mem::addr::AddrRange;
use crate::mem::{kernel_end_lma, paging};

mod buddy;
mod memblock;

pub fn init(memory_areas: &[MemoryArea]) {
    let mut memblock = MemblockSystem::new(memory_areas);
    let managed_range = memblock.managed_range().clone();
    init_remap(&mut memblock);

    // init PMM
    PMM.call_once(|| {
        // SAFETY: PhysicalRemap was mapped.
        let pmm = unsafe { PhysicalMemoryManager::new(managed_range, memblock) };
        spin::Mutex::new(pmm)
    });
}

fn init_remap(memblock: &mut MemblockSystem) {
    use paging::Flag;

    let managed_range = memblock.managed_range();
    let managed_pages = managed_range.overlapped_pages(PageSize::Huge);
    const PHYSICAL_REMAP_FLAGS: [Flag; 4] =
        [Flag::Present, Flag::Global, Flag::ReadWrite, Flag::PageSize];

    for ppage in managed_pages {
        let vpage = PageAddr::new(
            PhysicalRemapSpace::p2v(ppage.start()),
            ppage.size(),
        );
        unsafe {
            MMU.map(
                vpage,
                ppage,
                PHYSICAL_REMAP_FLAGS,
                memblock,
            )
            .expect("PhysicalRemap flags should be valid");
        }
    }
}



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

bitflags::bitflags! {
struct Flag: u8 {
}}

struct Frame {
    order: u8,
    flag: Flag,
}

impl Frame {
    fn order(self: Pin<&mut Self>) -> &mut u8 {
        // SAFETY: order field is not pinned.
        &mut unsafe { self.get_unchecked_mut() }.order
    }

    fn flag(self: Pin<&mut Self>) -> &mut Flag {
        // SAFETY: order field is not pinned.
        &mut unsafe { self.get_unchecked_mut() }.flag
    }
}


pub static PMM: spin::Once<spin::Mutex<PhysicalMemoryManager>> = spin::Once::new();

#[derive(Debug, Clone, Copy)]
pub struct Pfn(NonMaxUsize);

pub struct PhysicalMemoryManager {
    frames: &'static mut [Frame],
    base: PageAddr<LinearSpace>,
    buddy: BuddySystem,
}
impl PhysicalMemoryManager {
    /// Create a [`PhysicalMemoryManager`] for [`LinearSpace`]
    ///
    /// Since `PhysicalMemoryManager` does not track its own memory,
    /// its backing memory is leaked.
    ///
    /// # Safety
    /// PhysicalRemapSpace should be mapped.
    unsafe fn new(
        managed_range: Range<Addr<LinearSpace>>,
        mut memblock_system: MemblockSystem,
    ) -> Self {
        // SAFETY: Caller ensures PhysicalRemapSpace is mapped
        let boot_alloc = unsafe { MemblockAllocator::new(&mut memblock_system) };
        let managed_pages = managed_range.overlapped_pages(PageSize::Small);
        let frames_layout = Layout::array::<Frame>(managed_pages.len())
            .expect("Frame layout should not be too large");
        let frames_ptr = boot_alloc
            .allocate(frames_layout)
            .expect("Boot allocation should succeed");
        let mut frames_ptr = NonNull::slice_from_raw_parts(frames_ptr.cast(), managed_pages.len());

        // SAFETY: frames_ptr is allocated from frames_layout
        let frames = unsafe { frames_ptr.as_mut() };
        let base = PageAddr::new(managed_pages.start(), PageSize::Small);
        let mut buddy =
            BuddySystem::new(frames.len(), boot_alloc).expect("Boot Allocator should not fail.");

        let (free_blocks, _, _) = memblock_system.destroy();
        for free_block in free_blocks {
            for aligned in free_block.aligned_split(BUDDY_MAX_ORDER) {
                let idx =
                    aligned.base.addr_sub(managed_range.start) as usize / PageSize::Small.usize();
                let pfn = Pfn(NonMaxUsize::new(idx).unwrap());
                let order = aligned.base.usize().trailing_zeros() as u8;

                // SAFETY: Initializing buddy
                unsafe {
                    buddy.free_forced(pfn, order);
                }
            }
        }
        Self {
            frames,
            base,
            buddy,
        }
    }

    fn page(&self, pfn: Pfn) -> Option<Pin<&Frame>> {
        // SAFETY: All frames are pinned
        self.frames
            .get(pfn.0.get())
            .map(|x| unsafe { Pin::new_unchecked(x) })
    }

    fn page_mut(&mut self, pfn: Pfn) -> Option<Pin<&mut Frame>> {
        // SAFETY: All frames are pinned
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

    const fn pfn(&self, frame: &Frame) -> Pfn {
        let page_ptr = ptr::from_ref(frame);
        // SAFETY: Page structs are all located within PMM.frames
        let idx: isize = unsafe { page_ptr.offset_from(self.frames_ptr()) };
        let idx: usize = idx as usize;
        // SAFETY: idx cannot be too large
        Pfn(unsafe { NonMaxUsize::new_unchecked(idx) })
    }

    const fn frames_ptr(&self) -> *const Frame { &raw const self.frames[0] }
}

impl PageManager<LinearSpace> for PhysicalMemoryManager {
    fn allocate_pages(&mut self, cnt: usize, page_size: PageSize) 
        -> Option<PageRange<LinearSpace>> 
    {   
        let spage_cnt = cnt * (page_size.usize() / PageSize::Small.usize());
        let order = spage_cnt.next_power_of_two().ilog2();
        let pfn = self.buddy.reserve(order)?;
    }

    unsafe fn deallocate_pages(&mut self, pages: PageRange<LinearSpace>) {
        todo!()
    }
}

// ------------ arch ----------------


// ---------------------- misc ---------------------
/// Find the initial free memory areas.
///
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'boot>(
    boot_info: &'boot BootInformation,
) -> impl Iterator<Item = Range<Addr<LinearSpace>>> + 'boot {
    let mbi_range = unsafe {
        let start = Addr::new(boot_info.start_address());
        let end = Addr::new(boot_info.end_address());
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
            let start = Addr::new(area.start_address() as usize);
            let end = Addr::new(area.end_address() as usize);
            start..end
        })
        .flat_map(move |range| range.range_sub(kernel_area.clone()))
        .filter(|x| !x.is_empty())
        .flat_map(move |range| range.range_sub(mbi_range.clone()))
        .filter(|x| !x.is_empty())
}

/// Find the initial range of available physical memory
fn initial_memory_range(boot_info: &BootInformation) -> Range<Addr<LinearSpace>> {
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

    unsafe { Addr::new(min)..Addr::new(max + 1) }
}
