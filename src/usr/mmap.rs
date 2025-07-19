//! # Kernel Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0x0000000008048000:0x????????????????|Process Data + Heap        |       |
//! |0x????????????????:0x00007FFFC0000000|Stack                      |       |

use alloc::vec::Vec;
use core::hint::unreachable_unchecked;
use core::iter;

use crate::common::log::info;
use crate::mem::addr::{Addr, AddrRange, AddrSpace, Page, PageRange, PageSize};
use crate::mem::paging::{Attribute, MemoryManager, MemoryMap};
use crate::mem::{PageAllocator, Paging, UMASpace, UserSpace, MMU};



// Virtual memory mapping for user space.
pub struct MMap {
    regions: Vec<Region>,
    paging: Paging,
}

impl MMap {
    pub fn empty(paging: Paging) -> Self {
        Self {
            regions: Vec::new(),
            paging,
        }
    }
    fn find_available_range(&self, addr: Page<UserSpace>, size: usize) -> Option<usize> {
        let available_starts = self
            .regions
            .iter()
            .map(|x| x.range.base.checked_add(x.range.len).unwrap().addr());
        let available_starts = iter::once(UserSpace::MIN_ADDR).chain(available_starts);

        let available_ends = self.regions.iter().map(|x| x.range.base.addr());
        let available_ends = available_ends.chain(iter::once(UserSpace::MAX_ADDR));

        iter::zip(available_starts, available_ends)
            .enumerate()
            .skip_while(|(_, (_, end))| addr.addr() >= *end)
            .find(|&(_, (start, end))| {
                let start = start.max(addr.addr());
                debug_assert!(end >= start);
                (end.addr_sub(start) as usize) > size
            })
            .map(|x| x.0)
    }
    fn try_reserve_range(
        &mut self,
        addr: Option<Addr<UserSpace>>,
        size: usize,
        attr: Attribute,
    ) -> Option<PageRange<UserSpace>> {
        let addr = addr
            .unwrap_or(UserSpace::MIN_ADDR)
            .align_ceil(PageSize::MIN.usize())?;
        let addr = Page::new(addr, PageSize::MIN);
        let size = size.checked_next_multiple_of(PageSize::MIN.usize())?;

        let new_idx = self.find_available_range(addr, size)?;

        let page_cnt = size / PageSize::MIN.usize();
        let range = PageRange {
            base: addr,
            len: page_cnt,
        };

        let new_region = Region { range, attr };
        self.regions.insert(new_idx, new_region);
        Some(range)
    }

    pub fn activate(&self, cpu_id: u8) {
        let mut mmu = MMU.lock();
        let mmu = mmu.as_mut().unwrap();
        mmu.swap(self.paging.clone());
    }

    pub unsafe fn raw_map(
        &mut self,
        addr: Option<Addr<UserSpace>>,
        ppages: PageRange<UMASpace>,
        attr: Attribute,
    ) -> Option<PageRange<UserSpace>> {
        let vpages = self.try_reserve_range(
            addr,
            ppages.len * PageSize::MIN.usize(),
            attr,
        )?;
        info!(
            "MMapped {:#x} to {:#x} for {} pages",
            vpages.base.addr().usize(),
            ppages.base.addr().usize(),
            ppages.len
        );
        for (ppage, vpage) in iter::zip(ppages, vpages) {
            unsafe { self.paging.map(vpage, ppage, attr, &mut PageAllocator) };
        }
        Some(vpages)
    }
}

struct Region {
    range: PageRange<UserSpace>,
    attr: Attribute,
}
