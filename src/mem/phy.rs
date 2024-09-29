use core::{alloc::Layout, iter, marker::PhantomPinned, mem, ops::{Add, DerefMut}, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, slice, usize};

use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};
use derive_more::derive::{From, Into, Sub};
use multiboot2::{BootInformation, MemoryAreaTypeId};

use crate::mem::{kernel_end_lma, kernel_size, kernel_start_lma};

use super::{virt::VAddr, AddrRange};

const PAGE_SIZE: usize = 0x1000;
const _: () = assert!(PAGE_SIZE.is_power_of_two());

static BOOT_ALLOCATOR: Option<spin::Mutex<BootAllocator<PAGE_SIZE>>> = None;

/// Address in physical address space
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PAddr(usize);
pub trait FrameAllocator {
    fn page_sizes(&self) -> &[usize];
    fn allocate(&mut self, size: usize) -> Option<PAddr>;
    fn deallocate(&mut self, addr: PAddr);
}
/// Bootstrapping allocator to be used before the frame mapping allocator is
/// available
struct BootAllocator<'bootloader, const S: usize> {
    mbi_range: AddrRange,
    memory_areas: &'bootloader [multiboot2::MemoryArea],
    brk: usize,
    frame_sizes: [usize; 1],
}
impl<'bootloader, const S: usize> BootAllocator<'bootloader, S> {
    pub fn new<'a>(boot_info: &'a BootInformation ) -> BootAllocator<'a, S> {
        let mbi_range = AddrRange::from(
            boot_info.start_address() as usize .. boot_info.end_address() as usize
        );
        let memory_areas = boot_info.memory_map_tag().unwrap().memory_areas();
        BootAllocator { 
            mbi_range, 
            memory_areas,
            brk: 0,
            frame_sizes: [S],
        }
    }
    fn free_areas(&'bootloader self) -> impl Iterator<Item = AddrRange> + 'bootloader{
        let available: MemoryAreaTypeId = multiboot2::MemoryAreaType::Available.into();
        let kernel_area = AddrRange::from(kernel_start_lma() .. kernel_end_lma());
        self.memory_areas
            .iter()
            .filter(move |area| 
                area.typ() == available && area.end_address() as usize > self.brk
            )
            .map(|area| AddrRange::from(
                area.start_address() as usize .. area.end_address() as usize
            ))
            .flat_map(move |range| range - kernel_area)
            .flat_map(|range| range - self.mbi_range)
    }
}
impl<const S: usize> FrameAllocator for BootAllocator<'_, S> {
    fn page_sizes(&self) -> &[usize] {
        &self.frame_sizes
    }

    fn allocate(&mut self, size: usize) -> Option<PAddr> {
        assert!(size == S);
        fn find_aligned_page<const S: usize>(range: AddrRange)
            -> Option<usize> {
            let start = range.start.checked_next_multiple_of(S)?;
            let end = range.end - (range.end % S);
            (end.saturating_sub(start) >= S).then_some(start)
        }
        let addr = self.free_areas().find_map(find_aligned_page::<S>)?;
        self.brk = addr + S;
        Some(PAddr(addr))
    }
    
    fn deallocate(&mut self, _: PAddr) {
        // BootAllocator does not deallocate
    }

}

#[repr(C)]
struct FrameMap {
    /// Starting address of the memory which `FrameMap` manages. 
    /// It is guarenteed to be `PAGE_SIZE` aligned
    base: PAddr,
    /// Number of pages managed by the `FrameMap`
    len: usize,
    /// Bitmap stored as raw bytes that should be read as a `BitSlice`
    map: [u8]
}
impl FrameMap {
    const fn align_required() -> usize {
        align_of::<usize>()
    }
    /// Calculate the byte size of `FrameMap` for managing `page_cnt` pages
    const fn bytes_required(page_cnt: usize) -> usize {
        2 * size_of::<usize>() + match page_cnt {
            0 => 0,
            n => n.div_ceil(size_of::<u8>())
        }
    }
    /// Initialize a `FrameMap` at `addr` that is able to manage `page_cnt` pages
    /// starting at `base` and returning a mutable reference to the `FrameMap`. 
    /// The map is initially fully occupied.
    /// 
    /// # Safety
    /// Let `n_bytes_required` be returned by `bytes_required(page_cnt)`. 
    /// `addr` to `addr + n_bytes_required` must point to an unowned, valid 
    /// region of memory.
    unsafe fn init<'a>(addr: VAddr, page_cnt: usize, base: PAddr) -> &'a mut FrameMap {
        let n_bytes_required = Self::bytes_required(page_cnt);
        let map_bytes_required = n_bytes_required - 2 * size_of::<usize>();

        let map_ptr: *mut FrameMap = ptr::from_raw_parts_mut(
            usize::from(addr) as *mut u8, map_bytes_required
        );

        // SAFETY: FrameMap has repr(C), thus for a FrameMap with map size of
        // n, its layout is two usize followed by a slice of length n. 
        // n_bytes_required includes the usize fields as well, so subtracting
        // out two usize gives map_bytes_required. The memory region 
        // addr .. addr + n_bytes_required is guarenteed to be dereferencable
        // by the caller.
        let map = unsafe { map_ptr.as_mut_unchecked() };

        // The fields must be set manually because slice cannot be constructed
        map.base = base;
        map.len = page_cnt;
        map.map.fill(0xFF);
        map
    }

    /// Set the occupancy bit for the `page_cnt` pages starting with the page 
    /// pointed by `addr` 
    /// 
    /// # Safety
    /// - `addr` should be page aligned
    /// - `addr` to `addr + page_cnt * PAGE_SIZE` should be fully managed by
    /// the `FrameMap`
    unsafe fn set_unchecked(&mut self, addr: PAddr, page_cnt: usize, is_occupied: bool) {
        let PAddr(addr) = addr;
        let idx = usize::from(addr - self.base.0) / PAGE_SIZE;
        let slice = self.map.view_bits_mut::<Lsb0>();
        slice[idx..idx + page_cnt].fill(is_occupied);
    }

    /// Set the occupancy bit for all managed pages that overlaps with 
    /// `addr` to `addr + size`. Does not do anything if `addr + size` is out
    /// of bound. 
    fn set(&mut self, addr: PAddr, size: usize, is_occupied: bool) {
        let PAddr(addr) = addr;
        let Some(addr_end) = addr.checked_add(size) else { return; };
        let section_start = addr.max(self.base.0);
        let section_start = section_start - (section_start % PAGE_SIZE);

        let section_end = addr_end.min(self.base.0 + self.len * PAGE_SIZE);
        let section_end = section_end.next_multiple_of(PAGE_SIZE);

        // The provided section does not overlap with managed pages
        if section_start >= section_end {
            return;
        }

        let page_cnt = (section_end - section_start) / PAGE_SIZE;

        // SAFETY: section_start is page_aligned by construction. 
        // section_start >= self.base and 
        // section_end <= self.base + self.len * PAGE_SIZE, so 
        // section_start .. section_end is managed by the FrameMap
        unsafe { self.set_unchecked(PAddr(section_start), page_cnt, is_occupied); }
    }

    /// Returns address to the start of a section of unoccupied `page_cnt` 
    /// pages.
    /// 
    /// # Panics
    /// 
    /// This panics if `page_cnt == 0`
    fn find_unoccupied(&self, page_cnt: usize) -> Option<PAddr> {
        let slice = self.map.view_bits::<Lsb0>();
        let idx = slice.windows(page_cnt)
            .enumerate()
            .find(|(_, window)| 
                window.not_any()
            )?.0;
        Some(PAddr(self.base.0 + idx * PAGE_SIZE))
    }
}
