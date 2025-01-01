use core::{alloc::Layout, cell::{OnceCell, SyncUnsafeCell}, hash::BuildHasherDefault, iter, marker::{PhantomData, PhantomPinned}, mem, ops::{Add, DerefMut, Range}, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, slice, usize};

use alloc::vec::Vec;
use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};
use spin::Mutex;

use crate::{boot, common::TiB, mem::kernel_end_lma};

use super::{addr::{Addr, AddrRange as _, AddrSpace, PageAddr, PageManager, PageRange, PageSize,}, kernel_start_lma, memblock::BootMemoryManager, p2v, paging::{MemoryManager, X86_64MemoryManager}, virt::{BumpMemoryManager, VAllocSpace}};

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



type PhyId = usize;
struct PhysicalPage {
    next_idx: PhyId,
    prev_idx: PhyId,
    flag: u8,
}

struct PhysicalPageManager {
    pages: &'static mut [PhysicalPage],
}
impl PhysicalPageManager {
    fn new(
        boot_mm: &BootMemoryManager, 
        vmm: &BumpMemoryManager<VAllocSpace>, 
        mmu: &X86_64MemoryManager,
        range: PageRange<LinearSpace>
    ) -> Self {
        boot_mm.allocate_pages(cnt, page_size);
        vmm.allocate_pages(cnt, page_size)
    }
}


impl PageManager<LinearSpace> for PhysicalPageManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageAddr<LinearSpace>> {
        assert!(cnt == 1, "StackPageManager supports only one page");
        todo!()
    }

    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: PageAddr<LinearSpace>) -> Option<PageAddr<LinearSpace>> {
        assert!(cnt == 1, "StackPageManager supports only one page");
        todo!()
    }

    unsafe fn deallocate_pages(&self, page: PageAddr<LinearSpace>, cnt: usize) {
        assert!(cnt == 1, "StackPageManager supports only one page");
        todo!()
    }
}


impl PageManager<LinearSpace> for BootMemoryManager {
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageAddr<LinearSpace>> {
        let layout = Layout::from_size_align(
            cnt * page_size.usize(), 
            page_size.alignment()
        ).expect("page_size should convert to a valid layout");
        self.allocate(layout).map(|addr| PageAddr::new(addr, page_size))
    }

    fn allocate_pages_at(
        &self, 
        cnt: usize, 
        page_size: PageSize, 
        at: PageAddr<LinearSpace>
    ) -> Option<PageAddr<LinearSpace>> {
        let layout = Layout::from_size_align(
            cnt * page_size.usize(), 
            page_size.alignment()
        ).expect("page_size should convert to a valid layout");
        self.allocate_at(layout, at.start().usize())
            .map(|addr| PageAddr::new(addr, page_size))
    }

    unsafe fn deallocate_pages(&self, page: PageAddr<LinearSpace>, _cnt: usize) {
        unsafe {self.deallocate(page.start())};
    }
}