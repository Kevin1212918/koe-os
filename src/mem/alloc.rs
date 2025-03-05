use alloc::boxed::Box;
use alloc::collections::LinkedList;
use core::alloc::{AllocError, Allocator, GlobalAlloc, Layout};
use core::marker::PhantomData;
use core::mem::MaybeUninit;
use core::ops::Div;
use core::ptr::{self, slice_from_raw_parts, NonNull};
use core::slice;
use core::sync::atomic::{AtomicU8, AtomicUsize};

use bitvec::order::Lsb0;
use bitvec::slice::BitSlice;
use bitvec::view::BitView;
use intrusive_collections::LinkedListLink;

use super::addr::{Addr, PageManager, PageSize};
use super::paging::{Flag, MemoryManager};
use super::phy::PhysicalMemoryManager;
use super::virt::VirtSpace;
use super::LinearSpace;
use crate::mem::addr::{AddrRange, AddrSpace, PageRange};
use crate::mem::paging::MMU;
use crate::mem::phy::PMM;
use crate::mem::virt::PhysicalRemapSpace;

fn allocate_pages<V: VirtSpace>(
    mmu: &impl MemoryManager,
    vmm: &mut impl PageManager<V>,
    pmm: &mut impl PageManager<LinearSpace>,

    cnt: usize,
    page_size: PageSize,
) -> Result<NonNull<[u8]>, AllocError> {
    let vbase = vmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;
    let pbase = pmm.allocate_pages(cnt, page_size).ok_or(AllocError)?;

    let ptr = NonNull::new(vbase.base.addr().into_ptr())
        .expect("successfull virtual page allocation should not return null address");

    debug_assert!(vbase.len == cnt);
    debug_assert!(pbase.len == cnt);

    let flags = [Flag::Present, Flag::ReadWrite];

    for (vpage, ppage) in Iterator::zip(vbase.into_iter(), pbase.into_iter()) {
        unsafe {
            mmu.map(vpage, ppage, flags, pmm).expect("TODO: cleanup");
        }
    }

    Ok(NonNull::slice_from_raw_parts(
        ptr,
        page_size.usize(),
    ))
}

// TODO: Auto huge page
struct PageAllocator;
unsafe impl Allocator for PageAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(PageSize::MIN.align() % layout.align() == 0);
        let page_cnt = layout
            .size()
            .checked_next_multiple_of(PageSize::MIN.usize())
            .ok_or(AllocError)?
            .div(PageSize::MIN.usize());
        let page_size = PageSize::MIN;

        let mut pmm = PMM.get().ok_or(AllocError)?.lock();

        let prange = pmm.allocate_pages(page_cnt, page_size).ok_or(AllocError)?;
        debug_assert!(prange.len == page_cnt);

        let vbase = PhysicalRemapSpace::p2v(prange.base.addr());
        let vrange = AddrRange::new(vbase, page_cnt * page_size.usize());
        let vrange = PageRange::try_from_range(vrange, page_size)
            .expect("vbase and size should be page_aligned.");

        let ptr = NonNull::new(vbase.into_ptr())
            .expect("successfull virtual page allocation should not return null address");

        // TODO: Pass flags from caller.
        let flags = [Flag::Present, Flag::ReadWrite];
        for (vpage, ppage) in Iterator::zip(vrange.into_iter(), prange.into_iter()) {
            unsafe {
                MMU.map(vpage, ppage, flags, &mut pmm)
                    .expect("TODO: cleanup");
            }
        }

        Ok(NonNull::slice_from_raw_parts(
            ptr,
            page_size.usize(),
        ))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
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

        let mut pmm = PMM.get().expect("PMM should have been initialized").lock();
        unsafe { pmm.deallocate_pages(prange) };
    }
}

#[global_allocator]
static DUMMY_ALLOC: DummyAllocator = DummyAllocator;
struct DummyAllocator;
unsafe impl GlobalAlloc for DummyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { unimplemented!() }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unimplemented!() }
}

trait Item: Sized {
    const _ASSERT_ITEM_IS_ALIGNED_TO_SLAB_PAGE: () =
        assert!(Layout::new::<Self>().align() <= SLAB_PAGE.align());
}

struct Cache<T: Item> {
    empty_slabs: LinkedList<Box<Slab<T>>>,
    partial_slabs: LinkedList<Box<Slab<T>>>,
    full_slabs: LinkedList<Box<Slab<T>>>,
}

// TODO: PORT
// TODO: Atomic map

const SLAB_PAGE: PageSize = PageSize::Small;
#[repr(C)]
struct Slab<T: Item> {
    link: LinkedListLink,
    buf: Box<[u8; const { SLAB_PAGE.usize() }], PageAllocator>,
    _phantom: PhantomData<T>,
}

/// A slab
///
/// | MAP | PADDING | DATA |
impl<T: Item> Slab<T> {
    const MAP_SIZE: usize = const { (SLAB_PAGE.usize() / Self::SLOT_SIZE).div_ceil(8) };
    const MAP_START: usize = 0;
    const SLOTS_LEN: usize = {
        let residual_size = SLAB_PAGE.usize() - Self::SLOTS_START;
        residual_size / Self::SLOT_SIZE
    };
    const SLOTS_START: usize = const {
        let map_end = Self::MAP_SIZE;
        let slots_start = map_end.next_multiple_of(Self::SLOT_ALIGN);
        assert!(slots_start < SLAB_PAGE.usize());
        slots_start
    };
    const SLOT_ALIGN: usize = { Layout::new::<Self>().align() };
    const SLOT_SIZE: usize = { Layout::new::<Self>().pad_to_align().size() };

    fn map(&self) -> &BitSlice<u8, Lsb0> { &self.buf.view_bits()[0..Self::SLOTS_LEN] }

    fn map_mut(&mut self) -> &mut BitSlice<u8, Lsb0> {
        &mut self.buf.view_bits_mut()[0..Self::SLOTS_LEN]
    }

    fn slots(&self) -> &[MaybeUninit<T>] {
        // SAFETY: Self::SLOTS_START is smaller than page size, so the ptr is
        // valid.
        let slots_start = unsafe { (&raw const self.buf).byte_add(Self::SLOTS_START) };
        let slots_start = slots_start.cast::<MaybeUninit<T>>();
        // FIXME: Safety comment here.
        unsafe { slice::from_raw_parts(slots_start, Self::SLOT_SIZE) }
    }

    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        // SAFETY: Self::SLOTS_START is smaller than page size, so the ptr is
        // valid.
        let slots_start = unsafe { (&raw mut self.buf).byte_add(Self::SLOTS_START) };
        let slots_start = slots_start.cast::<MaybeUninit<T>>();
        // FIXME: Safety comment here.
        unsafe { slice::from_raw_parts_mut(slots_start, Self::SLOT_SIZE) }
    }

    fn new() -> Self {
        let slab_layout = PageSize::MIN.layout();
        let slab_ptr = PageAllocator
            .allocate(slab_layout)
            .expect("Memory allocation should succeed.")
            .cast();
        let slab_buf = unsafe { Box::from_non_null_in(slab_ptr, PageAllocator) };

        let mut slab = Slab {
            link: LinkedListLink::new(),
            buf: slab_buf,
            _phantom: PhantomData,
        };
        slab.map_mut().fill(false);
        slab
    }

    fn reserve(&mut self) -> Option<NonNull<T>> {
        let map = self.map_mut();
        let idx = map.first_zero()?;
        // SAFETY: idx was returned from first_zero
        unsafe { map.replace_unchecked(idx, true) };
        debug_assert!(idx < Self::SLOTS_LEN);
        let uninit = &mut self.slots_mut()[idx];
        NonNull::new(uninit.as_mut_ptr().cast())
    }

    unsafe fn free(&mut self, ptr: NonNull<T>) {
        debug_assert!(self.buf.as_mut_ptr_range().contains(&ptr.as_ptr().cast()));

        // SAFETY: The ptr was reserved from this slab as guarenteed by caller.
        let idx = unsafe { ptr.as_ptr().offset_from(self.slots().as_ptr().cast()) };
        let idx = idx as usize;

        debug_assert!(self.map()[idx]);
        self.map_mut().replace(idx, false);
    }
}
