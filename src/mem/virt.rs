//!
//! 
//! # Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0xFFFF888000000000:0xFFFFC88000000000|Physical Memory Remap      | 64 TB |
//! |0xFFFFC90000000000:0xFFFFE90000000000|VAlloc                     | 32 TB |
//! |0xFFFFFE8000000000:0xFFFFFE9000000000|Recursive Paging           | 0.5TB |
//! |0xFFFFFFFF80000000:0xFFFFFFFFFF600000|Kernel Text/Data           |       |

use core::{marker::PhantomData, sync::atomic::AtomicUsize};

use derive_more::derive::{Into, Sub};
use multiboot2::BootInformation;

use crate::mem::{addr::{AddrRange, PAddr, PRange}, phy, phy_to_virt};

use super::addr::{PageAllocator, PageBitmap, PageSize, VAddr, VPage, VPages, VRange};

pub trait VirtSpace {
    const RANGE: VRange;
}
pub struct VAllocSpace;
impl VirtSpace for VAllocSpace {
    const RANGE: VRange = {
        let start = unsafe{VAddr::from_usize(0xFFFF_C900_0000_0000)};
        let end = unsafe{VAddr::from_usize(0xFFFF_E900_0000_0000)};
        start .. end
    };
}

pub struct KernelSpace;
impl KernelSpace {
    pub fn v2p(vaddr: VAddr) -> PAddr {
        assert!(Self::RANGE.contains(&vaddr));
        
        PAddr::from(vaddr.addr_sub(Self::RANGE.start) as usize)
    }
}
impl VirtSpace for KernelSpace {
    const RANGE: VRange = {
        let start = unsafe{VAddr::from_usize(0xFFFF_FFFF_8000_0000)};
        let end = unsafe{VAddr::from_usize(0xFFFF_FFFF_FF60_0000)};
        start .. end
    };
}

pub struct PhysicalRemapSpace;
impl PhysicalRemapSpace {
    pub const OFFSET: usize = Self::RANGE.start.into_usize();
}
impl VirtSpace for PhysicalRemapSpace {
    const RANGE: VRange = {
        let start = unsafe{VAddr::from_usize(0xFFFF_8880_0000_0000)};
        let end = unsafe{VAddr::from_usize(0xFFFF_C880_0000_0000)};
        start .. end
    };
}

pub struct RecursivePagingSpace;
impl VirtSpace for RecursivePagingSpace {
    const RANGE: VRange = {
        let start = unsafe {VAddr::from_usize(0xFFFF_FE80_0000_0000)};
        let end = unsafe {VAddr::from_usize(0xFFFF_FE80_0000_0000)};
        start .. end
    };
}



pub struct BrkAllocator<S: VirtSpace> {
    brk: AtomicUsize,
    _space: PhantomData<S>,
}
impl <S: VirtSpace> PageAllocator<VAddr> for BrkAllocator<S> {
    fn allocate(&self, cnt: usize, page_size: PageSize) -> Option<VPage> {
        use core::sync::atomic::Ordering;

        let size = cnt.checked_mul(page_size.into_usize())?;
        let align = page_size.alignment();
        loop {
            let old_brk = self.brk.load(Ordering::Relaxed);
            let ret_addr = VAddr::from(old_brk.checked_next_multiple_of(align)?);

            if (S::RANGE.end.addr_sub(ret_addr) as usize) < size {
                return None;
            } 
            let new_brk = ret_addr.into_usize() + size;

            let res = self.brk.compare_exchange_weak(
                old_brk, 
                new_brk, 
                Ordering::Relaxed, 
                Ordering::Relaxed
            );
            if res.is_ok() {
                return Some(VPage::new(ret_addr, page_size))
            }
        }

    }
    
    fn allocate_at(&self, _: usize, _: PageSize, _: VPage) -> Option<VPage> {
        panic!("BrkAllocator does not implement allocate_at");
    }

    unsafe fn deallocate(&self, addr: VPage, cnt: usize) {
        use core::sync::atomic::Ordering;

        let old_brk = self.brk.load(Ordering::Relaxed);
        let new_brk = old_brk - (addr.size().into_usize() * cnt);
        let new_brk = new_brk - (new_brk % addr.size().into_usize());

        self.brk.compare_exchange(
            old_brk, 
            new_brk, 
            Ordering::Relaxed, 
            Ordering::Relaxed
        ).expect("BrkAllocator should only handle one dealloc at a time");
    }
    
}


// pub const BITMAP_ALLOCATOR_PAGE_SIZE: PageSize = PageSize::Small;
// type VPageBitMap = PageBitmap<{BITMAP_ALLOCATOR_PAGE_SIZE.into_usize()}, VAddr>;
// pub struct BitmapAllocator<S: VirtSpace> {
//     bitmap: spin::Mutex<&'static mut VPageBitMap>,
//     _space: PhantomData<S>,
// }

// impl<S: VirtSpace> BitmapAllocator<S> {
//     pub const PAGE_SIZE: PageSize = PageSize::Small;
//     fn new(boot_info: &BootInformation, palloc: impl phy::Allocator) -> Self {

//         fn initial_memory_range(boot_info: &BootInformation) -> PRange {
//             let memory_areas = boot_info.memory_map_tag()
//                 .expect("BootInformation should include memory map").memory_areas();

//             let (mut min, mut max) = (usize::MAX, 0);
//             for area in memory_areas {
//                 min = usize::min(area.start_address() as usize, min);
//                 max = usize::max(area.end_address() as usize, max);
//             }
//             assert!(min < max, "BootInformation memory map should not be empty");

//             unsafe {
//                 PAddr::from_usize(min) .. PAddr::from_usize(max+1)
//             }
//         }

//         let init_mem_page_cnt = initial_memory_range(boot_info)
//             .contained_pages(BITMAP_ALLOCATOR_PAGE_SIZE)
//             .len();

//         let bitmap_size = VPageBitMap::bytes_required(init_mem_page_cnt);
//         let bitmap_pages = palloc.allocate(bitmap_size, BITMAP_ALLOCATOR_PAGE_SIZE)
//             .expect("phy::Allocator should succeed");
//         let bitmap_addr = phy_to_virt(bitmap_pages.start());

//         let bitmap_ref = unsafe { 
//             PageBitmap::init(
//                 bitmap_addr.into_ptr(), 
//                 S::RANGE.start, 
//                 init_mem_page_cnt
//             ) 
//         };

//         Self {bitmap: spin::Mutex::new(bitmap_ref), _space: PhantomData} 
//     }
// }

// impl<S: VirtSpace> Allocator<S> for BitmapAllocator<S> {
//     fn allocate_page(&self, page_size: PageSize) -> Option<VPage> {
//         assert!(page_size == BITMAP_ALLOCATOR_PAGE_SIZE);

//         let base = self.bitmap.lock().find_unoccupied(1)?;
//         unsafe { self.bitmap.lock().set_unchecked(base, 1, true) };
//         Some(VPage::new(base, page_size))
//     }

//     fn allocate_contiguous(&self, size: usize, page_size: PageSize) -> Option<VPages> {
//         assert!(page_size == BITMAP_ALLOCATOR_PAGE_SIZE);

//         let page_byte_size = page_size.into_usize();

//         let page_cnt = size.div_ceil(page_byte_size);
//         let base = self.bitmap.lock().find_unoccupied(page_cnt)?;
//         unsafe { self.bitmap.lock().set_unchecked(base, page_cnt, true) };

//         let start_page = VPage::new(base, page_size);
//         let end_page = VPage::new(base.byte_add(page_cnt * page_byte_size), page_size);
//         Some(VPages::new(start_page, end_page))
//     }

//     unsafe fn deallocate(&self, page: VPage) {
//         self.bitmap.lock().set(page.start(), BITMAP_ALLOCATOR_PAGE_SIZE.into_usize(), false);
//     }
// }
