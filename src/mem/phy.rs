use alloc::alloc::Allocator;
use core::alloc::{AllocError, Layout};
use core::cell::RefCell;
use core::fmt::Write as _;
use core::ops::Range;
use core::pin::Pin;
use core::ptr::NonNull;
use core::usize;

use buddy::{BuddySystem, BUDDY_MAX_ORDER};
use memblock::MemblockSystem;
use multiboot2::{BootInformation, MemoryArea, MemoryAreaTypeId};

use super::addr::{self, Addr, AddrSpace, PageAddr, PageRange, PageSize};
use super::kernel_start_lma;
use super::paging::{MemoryManager, MMU};
use super::virt::PhysicalRemapSpace;
use crate::common::{hlt, TiB};
use crate::mem::addr::AddrRange;
use crate::mem::{kernel_end_lma, paging};

mod buddy;
mod memblock;

pub fn init_boot_mem(memory_areas: &[MemoryArea]) -> BootMemoryManager {
    BootMemoryManager(RefCell::new(memblock::init(
        memory_areas,
    )))
}
pub fn init(mut bmm: BootMemoryManager) {
    // init PMM
    PMM.call_once(|| {
        // SAFETY: PhysicalRemap was mapped.
        let pmm = unsafe { PhysicalMemoryRecord::new(&bmm) };
        spin::Mutex::new(pmm)
    });
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

static PMM: spin::Once<spin::Mutex<PhysicalMemoryRecord>> = spin::Once::new();
pub const FRAME_ORDER: u8 = PageSize::MIN.order();
pub const FRAME_SIZE: usize = PageSize::MIN.usize();

struct PhysicalMemoryRecord {
    frames: &'static mut [Frame],
    base: PageAddr<UMASpace>,
    buddy: BuddySystem,
}

impl PhysicalMemoryRecord {
    /// Create a [`PhysicalMemoryRecord`] for [`UMASpace`].
    ///
    /// `PhysicalMemoryRecord` inherits all the records from `bmm`.
    /// Consequently, this function freezes `bmm`.
    ///
    /// Since `PhysicalMemoryRecord` does not track its own memory,
    /// its backing memory is leaked.
    ///
    /// # Safety
    /// PhysicalRemapSpace should be mapped.
    unsafe fn new(bmm: &BootMemoryManager) -> Self {
        // SAFETY: Caller ensures PhysicalRemapSpace is mapped
        let managed_range = bmm.0.borrow().managed_range();
        let managed_pages = managed_range.overlapped_pages(PageSize::Small);
        let frames_layout = Layout::array::<Frame>(managed_pages.len)
            .expect("Frame layout should not be too large");
        let frames_ptr = bmm
            .allocate(frames_layout)
            .expect("Boot allocation should succeed");
        let mut frames_ptr = NonNull::slice_from_raw_parts(frames_ptr.cast(), managed_pages.len);

        // SAFETY: frames_ptr is allocated from frames_layout
        let frames = unsafe { frames_ptr.as_mut() };
        let base = managed_pages.base;
        let mut buddy =
            BuddySystem::new(frames.len(), bmm).expect("Boot Allocator should not fail.");

        bmm.0.borrow_mut().freeze();
        let memblock_system = bmm.0.borrow();
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

pub struct PhysicalMemoryManager;
impl PhysicalMemoryManager {
    pub fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageRange<UMASpace>> {
        // FIXME : Not safe!
        unsafe { PMM.get_unchecked() }
            .lock()
            .allocate_pages(cnt, page_size)
    }

    pub unsafe fn deallocate_pages(&self, pages: PageRange<UMASpace>) {
        unsafe {
            PMM.get()
                .expect("Deallocating unallocated frame")
                .lock()
                .deallocate_pages(pages);
        }
    }
}
unsafe impl addr::Allocator<UMASpace> for PhysicalMemoryManager {
    /// Attempt to allocate a page of physical memory that fits the layout. The
    /// returned page is guarenteed to be the smallest page which fits the
    /// layout.
    ///
    /// Use [`Self::allocate_pages`] to allocate pages directly.
    fn allocate(&self, layout: Layout) -> Option<AddrRange<UMASpace>> {
        debug_assert!(
            PMM.get().is_some(),
            "PhysicalMemoryRecord should be initialized"
        );
        let page = PageSize::fit(layout)?;
        self.allocate_pages(1, page).map(|r| r.into())
    }

    /// Deallocated a page starting at `addr`.
    ///
    /// Use [`Self::deallocate_pages`] for deallocating pages from
    /// `allocate_pages`.
    unsafe fn deallocate(&self, addr: Addr<UMASpace>, layout: Layout) {
        debug_assert!(
            PMM.get().is_some(),
            "PhysicalMemoryRecord should be initialized"
        );
        let page = PageSize::fit(layout).expect("layout should fit into an allocated page");
        let alloc_range = PageRange {
            base: PageAddr::new(addr, page),
            len: 1,
        };
        unsafe { self.deallocate_pages(alloc_range) }
    }
}

pub struct BootMemoryManager(RefCell<&'static mut MemblockSystem>);
impl BootMemoryManager {
    pub fn managed_range(&self) -> AddrRange<UMASpace> { self.0.borrow().managed_range() }
}
unsafe impl addr::Allocator<UMASpace> for BootMemoryManager {
    fn allocate(&self, layout: Layout) -> Option<AddrRange<UMASpace>> {
        let base = self.0.try_borrow_mut().ok()?.reserve(layout)?;
        let size = layout.size();
        Some(AddrRange { base, size })
    }

    unsafe fn deallocate(&self, _addr: Addr<UMASpace>, _layout: Layout) {
        unimplemented!("BootMemoryManager cannot deallocate");
    }
}

unsafe impl Allocator for BootMemoryManager {
    /// # Note
    ///
    /// This should not be used before `PhysicalRemapSpace` is initialized.
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        let paddr = self
            .0
            .try_borrow_mut()
            .map_err(|_| AllocError)?
            .reserve(layout)
            .ok_or(AllocError)?;
        let vaddr = PhysicalRemapSpace::p2v(paddr);

        let ptr = NonNull::new(vaddr.into_ptr::<u8>().cast()).ok_or(AllocError)?;
        Ok(NonNull::slice_from_raw_parts(
            ptr,
            layout.size(),
        ))
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        unimplemented!("BootMemoryManager cannot deallocate");
    }
}
