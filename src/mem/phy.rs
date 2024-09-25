use core::{alloc::Layout, marker::PhantomPinned, mem, ops::Add, pin::Pin, ptr::{self, slice_from_raw_parts_mut, NonNull}, usize};

use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};
use derive_more::derive::{From, Into, Sub};

use crate::mem::{kernel_end_lma, kernel_size, kernel_start_lma};

use super::virt::VAddr;

const PAGE_SIZE: usize = 0x1000;
const _: () = assert!(PAGE_SIZE.is_power_of_two());

static ALLOCATOR: Allocator<'static> = Allocator::new();

/// Address in physical address space
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PAddr(usize);

struct Allocator<'a>(spin::Once<spin::Mutex<AllocatorInner<'a>>>);
impl Allocator<'_> {
    const fn new() -> Self {
        Allocator(spin::Once::new())
    }
    /// Initialize allocator
    /// 
    /// # Safety
    /// - 
    unsafe fn init(&self, boot_info: &multiboot2::BootInformation) {
        let mem_areas = boot_info.memory_map_tag().unwrap().memory_areas();
        fn page_aligned_mem_range(mem_areas: &[multiboot2::MemoryArea]) -> (PAddr, PAddr) {
            let (mem_lo, mem_hi) = mem_areas.iter().fold((usize::MAX, 0), 
            |(min, max), area| {
                let min = (area.start_address() as usize).min(min);
                let max = (area.end_address() as usize).max(max);
                (min, max)
            });
            if usize::MAX - mem_lo < PAGE_SIZE || mem_hi < PAGE_SIZE {
                panic!()
            }

            let mem_lo = mem_lo.next_multiple_of(PAGE_SIZE);
            let mem_hi = mem_hi - (mem_hi % PAGE_SIZE);
            (PAddr(mem_lo), PAddr(mem_hi))
        }

        fn find_gap(
            mem_areas: &[multiboot2::MemoryArea], 
            gap_size: usize, 
            gap_align: usize
        ) -> Option<PAddr> {
            let available = multiboot2::MemoryAreaTypeId
                ::from(multiboot2::MemoryAreaType::Available);

            for area in mem_areas {
                if area.typ() != available {
                    continue;
                }
                let gap_start = area.start_address() as usize;
                let gap_start = gap_start.next_multiple_of(gap_align);
                let gap_end = area.end_address() as usize;
                if gap_end - gap_start < gap_size {
                    continue;
                }

                return Some(PAddr(gap_start));
            }
            None
        }

        fn disable_unavailable_areas(
            map: &mut FrameMap,
            mem_areas: &[multiboot2::MemoryArea],
        ) {
            // Mark all the available areas
            let available = multiboot2::MemoryAreaTypeId
                ::from(multiboot2::MemoryAreaType::Available);

            for area in mem_areas {
                if area.typ() != available {
                    continue;
                }
                let addr = area.start_address() as usize;
                let size = area.size() as usize;
                map.set(PAddr(addr), size, false);
            }
        }

        fn disable_mbi_area(
            map: &mut FrameMap,
            mbi: &multiboot2::BootInformation,
        ) {
            map.set(PAddr(mbi.start_address()), mbi.total_size(), true);
        }

        fn disable_kernel_area(map: &mut FrameMap) {
            let size = kernel_end_lma() - kernel_start_lma();
            map.set(kernel_start_lma(), kernel_size(), true);
        }


        let (mem_lo, mem_hi) = page_aligned_mem_range(mem_areas);
        let page_cnt = (mem_hi.0 - mem_lo.0) / PAGE_SIZE;
        let map_bytes = FrameMap::bytes_required(page_cnt);
        let map_align = FrameMap::align_required();
        let map_addr = find_gap(mem_areas, map_bytes, map_align).unwrap();

        // Converting PAddr to VAddr because we are currently have identity paging

        // SAFETY: map_addr .. map_addr + map_bytes is page aligned and available
        // as indicated by boot info. The memory be marked as occupied later
        // in the function, and not allocated out
        let map = unsafe { FrameMap::init(map_addr, page_cnt, mem_lo) };

        // Populate frame map with unusable memories
        disable_unavailable_areas(map, mem_areas);
        disable_mbi_area(map, boot_info);
        disable_kernel_area(map);

        self.0.call_once(|| {
            spin::Mutex::new(AllocatorInner { map })
        });
    }

    /// Attempts to allocate `page_cnt` pages of physical memory. 
    /// 
    /// On success, returns a page aligned `PAddr` which points to the 
    /// start of a `page_cnt * PAGE_SIZE` sized block of allocated physical
    /// memory.
    pub fn allocate(&mut self, page_cnt: usize) -> Option<PAddr> {
        let mut inner = self.0.get_mut().unwrap().lock();
        let addr = inner.map.find_unoccupied(page_cnt)?;

        // Safety: Return from FrameMap::find_unoccupied is page aligned and 
        // points to the start of page_cnt pages
        unsafe { inner.map.set_unchecked(addr, page_cnt, true); }

        Some( addr.into() )
    }

    /// Deallocate the memory pointed by `ptr`
    /// 
    /// # Safety
    /// `ptr` points to `page_cnt` pages of physical memory currently 
    /// allocated through this allocator.
    pub unsafe fn deallocate(&mut self, ptr: PAddr, page_cnt: usize) {
        let mut inner = self.0.get_mut().unwrap().lock();
        // SAFETY: ptr is page aligned and points to page_cnt pages allocated
        // in this FrameMap
        unsafe { inner.map.set_unchecked(ptr.into(), page_cnt, false); }
    } 

}

/// Inner struct for `Allocator` holding the data. See `Allocator` for details.
struct AllocatorInner<'a> {
    map: &'a mut FrameMap,
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