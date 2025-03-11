use alloc::alloc::Allocator;
use core::alloc::Layout;
use core::fmt::Write as _;
use core::ops::Range;
use core::pin::Pin;
use core::ptr::NonNull;
use core::usize;

use buddy::{BuddySystem, BUDDY_MAX_ORDER};
use memblock::{MemblockAllocator, MemblockSystem};
use multiboot2::{BootInformation, MemoryArea, MemoryAreaTypeId};

use super::addr::{Addr, AddrSpace, PageAddr, PageManager, PageRange, PageSize};
use super::kernel_start_lma;
use super::paging::{MemoryManager, MMU};
use super::virt::PhysicalRemapSpace;
use crate::common::TiB;
use crate::mem::addr::AddrRange;
use crate::mem::{kernel_end_lma, paging};

mod buddy;
mod memblock;

pub fn init(memory_areas: &[MemoryArea]) {
    let mut memblock = memblock::init(memory_areas);
    let managed_range = memblock.managed_range().clone();
    init_remap(memblock.as_mut().get_mut());

    // init PMM
    PMM.call_once(|| {
        // SAFETY: PhysicalRemap was mapped.
        let pmm = unsafe { PhysicalMemoryManager::new(managed_range, memblock.get_mut()) };
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
            ppage.page_size(),
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
pub struct UMASpace;
impl PhySpace for UMASpace {}
impl AddrSpace for UMASpace {
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
pub const FRAME_ORDER: u8 = PageSize::MIN.order();
pub const FRAME_SIZE: usize = PageSize::MIN.usize();

pub struct PhysicalMemoryManager {
    frames: &'static mut [Frame],
    base: PageAddr<UMASpace>,
    buddy: BuddySystem,
}
impl PhysicalMemoryManager {
    /// Create a [`PhysicalMemoryManager`] for [`UMASpace`]
    ///
    /// Since `PhysicalMemoryManager` does not track its own memory,
    /// its backing memory is leaked.
    ///
    /// # Safety
    /// PhysicalRemapSpace should be mapped.
    unsafe fn new(
        managed_range: AddrRange<UMASpace>,
        memblock_system: &mut MemblockSystem,
    ) -> Self {
        // SAFETY: Caller ensures PhysicalRemapSpace is mapped
        let boot_alloc = unsafe { MemblockAllocator::new(memblock_system) };
        let managed_pages = managed_range.overlapped_pages(PageSize::Small);
        let frames_layout = Layout::array::<Frame>(managed_pages.len)
            .expect("Frame layout should not be too large");
        let frames_ptr = boot_alloc
            .allocate(frames_layout)
            .expect("Boot allocation should succeed");
        let mut frames_ptr = NonNull::slice_from_raw_parts(frames_ptr.cast(), managed_pages.len);

        // SAFETY: frames_ptr is allocated from frames_layout
        let frames = unsafe { frames_ptr.as_mut() };
        let base = managed_pages.base;
        let mut buddy =
            BuddySystem::new(frames.len(), boot_alloc).expect("Boot Allocator should not fail.");

        memblock_system.freeze();
        let free_blocks = memblock_system.free_blocks();
        for free_block in free_blocks {
            for aligned in free_block.aligned_split(
                FRAME_ORDER,
                BUDDY_MAX_ORDER + FRAME_ORDER,
            ) {
                assert!(aligned.base.is_aligned_to(FRAME_SIZE));
                let idx = (aligned.base - base.addr()) as usize / FRAME_SIZE;
                let block_order = aligned.size.trailing_zeros() as u8;
                let order = block_order - FRAME_ORDER;

                // SAFETY: Initializing buddy
                unsafe {
                    buddy.free_forced(idx, order);
                }
            }
        }
        Self {
            frames,
            base,
            buddy,
        }
    }

    fn frame(&self, addr: impl Into<Addr<UMASpace>>) -> Option<&Frame> {
        self.frame_idx(addr.into()).map(|idx| &self.frames[idx])
    }

    fn frame_mut(&mut self, addr: impl Into<Addr<UMASpace>>) -> Option<&mut Frame> {
        self.frame_idx(addr.into()).map(|idx| &mut self.frames[idx])
    }

    fn frame_idx(&self, addr: Addr<UMASpace>) -> Option<usize> {
        let byte_offset: usize = (addr - self.base.addr()).try_into().ok()?;
        let idx = byte_offset >> FRAME_ORDER;
        (idx < self.frames.len()).then_some(idx)
    }

    const fn frames_ptr(&self) -> *const Frame { &raw const self.frames[0] }
}

impl PageManager<UMASpace> for PhysicalMemoryManager {
    fn allocate_pages(&mut self, cnt: usize, page_size: PageSize) -> Option<PageRange<UMASpace>> {
        let frame_cnt = cnt * (page_size.usize() / FRAME_SIZE);
        let allocate_cnt = frame_cnt.next_power_of_two();
        let order = allocate_cnt.ilog2() as u8;
        if order > self.buddy.max_order() {
            return None;
        }

        let frame_idx = self.buddy.reserve(order)?;
        self.frames[frame_idx].order = order;

        let base = self
            .base
            .checked_page_add(frame_idx)
            .expect("index returned by buddy system should be correctly sized");
        let base = PageAddr::new(base.addr(), page_size);

        let len = allocate_cnt >> (page_size.order() - FRAME_ORDER);
        Some(PageRange { base, len })
    }

    unsafe fn deallocate_pages(&mut self, pages: PageRange<UMASpace>) {
        let frame_idx = self
            .frame_idx(pages.base.into())
            .expect("pages should be valid when deallocating");
        let frame_order = self.frames[frame_idx].order;
        // SAFETY: Guarenteed by caller to be allocated from buddy.
        unsafe {
            self.buddy.free(frame_idx, frame_order);
        }
        self.frames[frame_idx].order = 0;
    }
}

// ------------ arch ----------------


// ---------------------- misc ---------------------
/// Find the initial free memory areas.
///
/// Note this only considers the memory usage before `kmain` is called.
fn initial_free_memory_areas<'boot>(
    boot_info: &'boot BootInformation,
) -> impl Iterator<Item = AddrRange<UMASpace>> + 'boot {
    let mbi_range = {
        let start = Addr::new(boot_info.start_address());
        let end = Addr::new(boot_info.end_address());
        AddrRange::from(start..end)
    };
    let memory_areas = boot_info
        .memory_map_tag()
        .expect("BootInformation should include memory map")
        .memory_areas();

    let available: MemoryAreaTypeId = multiboot2::MemoryAreaType::Available.into();
    let kernel_area: AddrRange<UMASpace> = (kernel_start_lma()..kernel_end_lma()).into();
    memory_areas
        .iter()
        .filter(move |area| area.typ() == available)
        .map(|area| unsafe {
            let start = Addr::new(area.start_address() as usize);
            let end = Addr::new(area.end_address() as usize);
            AddrRange::from(start..end)
        })
        .flat_map(move |range| range.range_sub(kernel_area.clone()))
        .filter(|x| !x.is_empty())
        .flat_map(move |range| range.range_sub(mbi_range.clone()))
        .filter(|x| !x.is_empty())
}

/// Find the initial range of available physical memory
fn initial_memory_range(boot_info: &BootInformation) -> Range<Addr<UMASpace>> {
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
