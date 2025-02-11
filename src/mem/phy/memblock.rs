use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::SyncUnsafeCell;
use core::ops::{Add, Deref, Range};
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;

use arrayvec::ArrayVec;
use bitflags::bitflags;
use multiboot2::{MemoryArea, MemoryAreaType};

use crate::mem::addr::{Addr, AddrSpace, PageAddr};
use crate::mem::paging::{MemoryManager, PageTableAllocator};
use crate::mem::virt::PhysicalRemapSpace;
use crate::mem::{self, LinearSpace};

#[derive(Debug, Clone, Copy)]
enum MemTyp {
    Free,
    Reserved,
}

/// An extent of memory used in `BootMemoryManager``. Note present `Memblock`
/// is considered greater than not present `Memblock`
#[derive(Debug, Clone, Copy)]
struct Memblock {
    pub base: Addr<LinearSpace>,
    pub size: usize,
    pub typ: MemTyp,
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
struct Memblocks {
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

    // /// Find a block at `at` and cut off a `layout` fitted block. If such block
    // /// is not found at `at`, try find such block elsewhere.
    // fn cut_block(&mut self, layout: Layout) -> Option<Memblock> {
    //     fn find_cut_block_in_range(data: &[Memblock], layout: Layout) ->
    // Option<(usize, usize)> {         data.iter().enumerate().find_map(|(idx,
    // b)| {             let unaligned_base = b.base +
    // (b.size.checked_sub(layout.size())?);             let aligned_base =
    // unaligned_base - unaligned_base % layout.align();             
    // (aligned_base >= b.base).then_some((idx, aligned_base))         })
    //     }

    //     let (cut_idx, cut_base) = find_cut_block_in_range(&self.data, layout)?;

    //     let residual_size = cut_base - self.data[cut_idx].base;
    //     let base = cut_base;
    //     let size = self.data[cut_idx].size - residual_size;
    //     let typ = self.data[cut_idx].typ;

    //     if residual_size != 0 {
    //         self.data[cut_idx].size = residual_size;
    //     } else {
    //         self.data.remove(cut_idx);
    //     }

    //     Some(Memblock {base, size, typ })
    // }

    fn remove(&mut self, base: Addr<LinearSpace>) -> Option<Memblock> {
        let idx = self.data.binary_search_by_key(&base, |b| b.base).ok()?;

        Some(self.data.remove(idx))
    }

    fn pop(&mut self) -> Option<Memblock> { self.data.pop() }
}

struct MemblockSystem {
    free_blocks: Memblocks,
    reserved_blocks: Memblocks,
    partial_block: Option<Memblock>,
    offset: usize,
    managed_range: Range<Addr<LinearSpace>>,
}
impl MemblockSystem {
    fn new<T>(memory: &[T]) -> Self
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

    fn reserve(&mut self, layout: Layout) -> Option<Addr<LinearSpace>> {
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

    fn free(&mut self, layout: Layout) { unimplemented!("MemblockSystem cannot free!") }
}

//------------------- arch ------------------------

struct MemblockAllocator<'mmu, T: MemoryManager> {
    memblock: spin::Mutex<MemblockSystem>,
    mmu: &'mmu T,
    high_mark: AtomicUsize,
}
impl<'mmu, T: MemoryManager> MemblockAllocator<'mmu, T> {
    pub fn new<U>(memory: &[U], mmu: &'mmu T) -> Self
    where
        Memblock: for<'a> From<&'a U>,
    {
        let memblock = spin::Mutex::new(MemblockSystem::new(memory));
        let high_mark = AtomicUsize::new(0);
        Self {
            memblock,
            mmu,
            high_mark,
        }
    }
}
unsafe impl<'mmu, T: MemoryManager> Allocator for MemblockAllocator<'mmu, T> {
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
