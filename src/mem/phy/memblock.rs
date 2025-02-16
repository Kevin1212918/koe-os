use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::ops::{Add, Range};
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;

use arrayvec::ArrayVec;
use derive_more::derive::IntoIterator;
use multiboot2::{MemoryArea, MemoryAreaType};

use crate::mem::addr::{Addr, AddrSpace, PageAddr, PageManager, PageRange, PageSize};
use crate::mem::paging::MemoryManager;
use crate::mem::virt::PhysicalRemapSpace;
use crate::mem::LinearSpace;

#[derive(Debug, Clone, Copy)]
enum MemTyp {
    Free,
    Reserved,
}

/// An extent of memory used in `BootMemoryManager``. Note present `Memblock`
/// is considered greater than not present `Memblock`
#[derive(Debug, Clone, Copy)]
pub struct Memblock {
    pub base: Addr<LinearSpace>,
    pub size: usize,
    typ: MemTyp,
}
impl Memblock {
    pub fn aligned_split(self, max_order: u8) -> AlignedSplit {
        AlignedSplit {
            memblock: self,
            offset: 0,
            max_order: max_order as u32,
        }
    }

    pub fn order(&self) -> u8 { self.base.usize().trailing_zeros() as u8 }
}
impl PartialEq for Memblock {
    fn eq(&self, other: &Self) -> bool { self.base == other.base }
}
impl Eq for Memblock {}
impl PartialOrd for Memblock {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.base.partial_cmp(&other.base)
    }
}
impl Ord for Memblock {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering { self.base.cmp(&other.base) }
}

impl From<&MemoryArea> for Memblock {
    fn from(value: &MemoryArea) -> Self {
        let ma_typ: MemoryAreaType = value.typ().into();
        let typ = match ma_typ {
            MemoryAreaType::Available => MemTyp::Free,
            MemoryAreaType::Reserved
            | MemoryAreaType::AcpiAvailable
            | MemoryAreaType::ReservedHibernate
            | MemoryAreaType::Defective
            | MemoryAreaType::Custom(_) => MemTyp::Reserved,
        };
        Memblock {
            base: Addr::new(value.start_address() as usize),
            size: value.size() as usize,
            typ,
        }
    }
}


const MEMBLOCKS_LEN: usize = 128;
/// A reference to a sorted array of `Memblock`s.
#[derive(IntoIterator)]
pub struct Memblocks {
    #[into_iterator(owned, ref)]
    data: ArrayVec<Memblock, MEMBLOCKS_LEN>,
}
impl Memblocks {
    fn new() -> Self {
        Self {
            data: ArrayVec::new(),
        }
    }

    fn insert(&mut self, block: Memblock) -> bool {
        let Some(pivot) = self.data.binary_search(&block).err() else {
            // Memblocks should not overlap
            return false;
        };

        // Check if block overlap or merge with prev or next block
        let mut merge_prev: Option<usize> = None;
        let mut merge_next: Option<usize> = None;

        if pivot > 0 {
            // SAFETY: Given pivot > 0, pivot - 1 should be in bound
            let prev = unsafe { self.data.get_unchecked(pivot - 1) };
            debug_assert!(prev.base <= block.base);
            if block.base.addr_sub(prev.base) as usize == prev.size {
                merge_prev = Some(pivot - 1)
            };
        }

        // SAFETY: pivot should be in bound
        let next = unsafe { self.data.get_unchecked(pivot) };
        debug_assert!(next.base >= block.base);
        if next.base.addr_sub(block.base) as usize == block.size {
            merge_next = Some(pivot);
        }

        match (merge_prev, merge_next) {
            (None, None) => self.data.insert(pivot, block),
            (None, Some(idx)) => {
                self.data[idx].base = block.base;
                self.data[idx].size += block.size;
            },
            (Some(idx), None) => {
                self.data[idx].size += block.size;
            },
            (Some(prev), Some(pivot)) => {
                self.data[prev].size += self.data[pivot].size + block.size;
                self.data.remove(pivot);
            },
        }

        true
    }

    fn remove(&mut self, base: Addr<LinearSpace>) -> Option<Memblock> {
        let idx = self.data.binary_search_by_key(&base, |b| b.base).ok()?;

        Some(self.data.remove(idx))
    }

    fn pop(&mut self) -> Option<Memblock> { self.data.pop() }

    fn iter(&self) -> impl Iterator<Item = &Memblock> { self.data.iter() }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Memblock> { self.data.iter_mut() }
}

pub struct MemblockSystem {
    free_blocks: Memblocks,
    reserved_blocks: Memblocks,
    partial_block: Option<Memblock>,
    offset: usize,
    managed_range: Range<Addr<LinearSpace>>,
}
impl MemblockSystem {
    pub fn new<T>(memory: &[T]) -> Self
    where
        Memblock: for<'a> From<&'a T>,
    {
        let mut free_blocks = Memblocks::new();
        let mut reserved_blocks = Memblocks::new();

        let mut min_addr: Addr<LinearSpace> = Addr::new(LinearSpace::RANGE.end);
        let mut max_addr: Addr<LinearSpace> = Addr::new(LinearSpace::RANGE.start);

        for block in memory.iter().map(|x| Memblock::from(x)) {
            min_addr = min_addr.min(block.base);
            max_addr = max_addr.max(block.base.byte_add(block.size));

            match block.typ {
                MemTyp::Free => free_blocks.insert(block),
                MemTyp::Reserved => reserved_blocks.insert(block),
            };
        }

        let partial_block = free_blocks.pop();
        let offset = 0;

        Self {
            free_blocks,
            reserved_blocks,
            partial_block,
            offset,
            managed_range: min_addr..max_addr,
        }
    }

    pub fn reserve(&mut self, layout: Layout) -> Option<Addr<LinearSpace>> {
        let partial_block = &mut self.partial_block?;
        self.offset = self.offset.next_multiple_of(layout.align());
        let base = self.offset;
        self.offset += layout.size();

        if self.offset > partial_block.size {
            self.reserved_blocks.insert(*partial_block);
            self.partial_block = self.free_blocks.pop();
            self.offset = 0;
            self.reserve(layout)
        } else {
            Some(Addr::new(base))
        }
    }

    /// Destroys `Memblock` system and returns (free blocks, reserved blocks,
    /// managed range)
    pub fn destroy(
        mut self,
    ) -> (
        Memblocks,
        Memblocks,
        Range<Addr<LinearSpace>>,
    ) {
        if self.offset != 0 && self.partial_block.is_some() {
            // cut partial block to reserved and free
            let partial_block = self.partial_block.unwrap();
            let reserved_base = partial_block.base;
            let reserved_size = self.offset;
            let free_base = reserved_base.byte_add(reserved_size);
            let free_size = partial_block.size - reserved_size;

            self.free_blocks.insert(Memblock {
                base: free_base,
                size: free_size,
                typ: MemTyp::Free,
            });
            self.reserved_blocks.insert(Memblock {
                base: reserved_base,
                size: reserved_size,
                typ: MemTyp::Reserved,
            });
        }
        (
            self.free_blocks,
            self.reserved_blocks,
            self.managed_range,
        )
    }

    pub fn managed_range(&self) -> Range<Addr<LinearSpace>> { self.managed_range.clone() }
}

/// An iterator of power-of-2 aligned memblocks splitted from a single memblock.
pub struct AlignedSplit {
    memblock: Memblock,
    offset: usize,
    max_order: u32,
}
impl Iterator for AlignedSplit {
    type Item = Memblock;

    fn next(&mut self) -> Option<Self::Item> {
        // NOTE: Optimize this shit
        if self.offset == self.memblock.size {
            return None;
        }
        let offset_order = self.offset.trailing_zeros();
        let diff_order = (self.memblock.size - self.offset).trailing_zeros();
        let next_order = offset_order.min(diff_order).min(self.max_order);

        let next_size = 1 << next_order;
        let next = Memblock {
            base: self.memblock.base.byte_add(self.offset),
            size: next_size,
            typ: self.memblock.typ,
        };
        self.offset += next_size;
        Some(next)
    }
}



//------------------- arch ------------------------

impl PageManager<LinearSpace> for MemblockSystem {
    fn allocate_pages(
        &mut self,
        cnt: usize,
        page_size: PageSize,
    ) -> Option<PageRange<LinearSpace>> {
        let layout = Layout::from_size_align(
            cnt * page_size.usize(),
            page_size.alignment(),
        )
        .expect("Layout for a page range should be valid");
        let addr = self.reserve(layout)?;
        Some(PageRange::new(
            PageAddr::new(addr, page_size),
            cnt,
        ))
    }

    unsafe fn deallocate_pages(&mut self, _pages: PageRange<LinearSpace>) {
        unimplemented!("MemblockAllocator cannot deallocate");
    }
}

pub struct MemblockAllocator<'b> {
    memblock: spin::Mutex<&'b mut MemblockSystem>,
    high_mark: AtomicUsize,
}
impl<'b> MemblockAllocator<'b> {
    /// Create a `MemblockAllocator`
    ///
    /// # Safety
    /// PhysicalRemapSpace should be mapped.
    pub unsafe fn new<U>(memblock: &'b mut MemblockSystem) -> Self
    where
        Memblock: for<'a> From<&'a U>,
    {
        let high_mark = AtomicUsize::new(0);
        Self {
            memblock: spin::Mutex::new(memblock),
            high_mark,
        }
    }

    pub fn managed_range(&self) -> Range<Addr<LinearSpace>> {
        self.memblock.lock().managed_range.clone()
    }
}

unsafe impl<'b> Allocator for MemblockAllocator<'b> {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        use core::sync::atomic::Ordering;

        let paddr = self.memblock.lock().reserve(layout).ok_or(AllocError)?;
        let vaddr = PhysicalRemapSpace::p2v(paddr);
        assert!(paddr.usize() >= self.high_mark.load(Ordering::Relaxed));

        let ptr = NonNull::new(vaddr.into_ptr::<u8>().cast()).ok_or(AllocError)?;
        Ok(NonNull::slice_from_raw_parts(
            ptr,
            layout.size(),
        ))
    }

    unsafe fn deallocate(&self, _ptr: NonNull<u8>, _layout: Layout) {
        unimplemented!("MemblockAllocator cannot deallocate");
    }
}
