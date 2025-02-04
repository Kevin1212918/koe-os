use core::{alloc::Layout, cell::SyncUnsafeCell};

use crate::mem::{self, addr::Addr, LinearSpace};

/// An extent of memory used in `BootMemoryManager``. Note present `Memblock` 
/// is considered greater than not present `Memblock`
#[derive(Debug, Default, Clone, Copy)]
struct Memblock {
    is_present: usize,
    base: usize,
    size: usize,
}
impl PartialEq for Memblock {
    fn eq(&self, other: &Self) -> bool {
        self.is_present == other.is_present && 
        (self.is_present == 0 || self.base == other.base)
    }
}
impl Eq for Memblock {}
impl PartialOrd for Memblock {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        match self.is_present.partial_cmp(&other.is_present) {
            Some(core::cmp::Ordering::Equal) => {}
            ord => return ord.map(|x|x.reverse()),
        }
        if self.is_present == 0 { return Some(core::cmp::Ordering::Equal); }
        self.base.partial_cmp(&other.base)
    }
}
impl Ord for Memblock {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.partial_cmp(other).expect("Memblock order should be total")
    }
}
impl Memblock {
    fn is_empty(&self) -> bool { self.is_present == 0 }
    const fn empty() -> Self { Self { is_present: 0, base: 0, size: 0 } }
    const fn new(base: usize, size: usize) -> Self { 
        Self {is_present: 1, base, size }
    }
}

/// A reference to a sorted array of `Memblock`s.
struct Memblocks {
    data: &'static mut [Memblock], 
    len: usize,
}
impl Memblocks {
    fn new(blocks: &'static mut [Memblock]) -> Self {
        blocks.sort_unstable();
        Self {data: blocks, len: 0}
    }
    fn len(&self) -> usize {
        self.len
    }
    fn put_block(&mut self, block: Memblock) -> bool {
        // Memblocks is full
        if self.len == self.data.len() { return false; }
        // Inserting empty block is no-op
        if block.is_empty() { return true; }

        let Some(pivot) = self.data.binary_search(&block).err() else {
            // Inserting overlapping block
            return false; 
        };

        // Check if block overlap or merge with prev or next block
        let mut merge_prev: Option<usize> = None;
        let mut merge_next: Option<usize> = None;

        if pivot > 0 {
            // SAFETY: Given pivot > 0, pivot - 1 should be in bound
            let prev = unsafe {self.data.get_unchecked(pivot - 1)};
            debug_assert!(!prev.is_empty());
            debug_assert!(prev.base <= block.base);
            if block.base - prev.base < prev.size { return false; }
            if block.base - prev.base == prev.size { merge_prev = Some(pivot - 1)};
        }

        // SAFETY: pivot should be in bound
        let next = unsafe {self.data.get_unchecked(pivot)};
        debug_assert!(next.is_empty() || next.base >= block.base);
        if !next.is_empty() {
            if next.base - block.base < block.size { return false; }
            if next.base - block.base == block.size { merge_next = Some(pivot); }
        }

        match (merge_prev, merge_next) {
            (None, None) => {
                self.data[pivot ..= self.len].rotate_right(1);
                self.data[pivot] = block;
                self.len += 1;
            },
            (None, Some(idx)) => {
                self.data[idx].base = block.base;
                self.data[idx].size += block.size;
            },
            (Some(idx), None) => {
                self.data[idx].size += block.size;
            },
            (Some(prev), Some(pivot)) => {
                debug_assert!(pivot != self.len);
                self.data[prev].size += self.data[pivot].size + block.size;
                self.data[pivot] = Memblock::empty();
                self.data[pivot .. self.len].rotate_left(1);
                self.len -= 1;
            },
        }

        true
    }

    /// Find a block at `at` and cut off a `layout` fitted block. If such block
    /// is not found at `at`, try find such block elsewhere.
    fn cut_block(&mut self, layout: Layout, at: Option<usize>) -> Option<Memblock> {

        fn find_cut_block_in_range(
            data: &[Memblock], 
            layout: Layout
        ) -> Option<(usize, usize)> {
            data.iter().enumerate()
                .find_map(|(idx, b)| {
                    if b.is_empty() { return None }
                    let unaligned_base = b.base + (b.size.checked_sub(layout.size())?);
                    let aligned_base = unaligned_base - unaligned_base % layout.align();
                    (aligned_base >= b.base).then_some((idx, aligned_base))
                })
        }

        let (cut_idx, cut_base) = match at {
            // If at is defined, find the index of the block which would
            // contain at
            Some(at) => {
                let at_idx = self.data.binary_search_by_key(&at, |b| {
                    if b.is_empty() { usize::MAX } else { b.base }
                }).map_or_else(|x| x, |x| x);

                find_cut_block_in_range(&self.data[at_idx .. self.len], layout).or_else(||
                find_cut_block_in_range(&self.data[0 .. at_idx], layout))?
            },
            None => find_cut_block_in_range(&self.data, layout)?,
        };
            
        let residual_size = cut_base - self.data[cut_idx].base;
        let base = cut_base;
        let size = self.data[cut_idx].size - residual_size;

        if residual_size != 0 {
            self.data[cut_idx].size = residual_size;
        } else {
            self.data[cut_idx] = Memblock::empty();
            self.data[cut_idx .. self.len].rotate_left(1);
            self.len -= 1;
        }

        Some(Memblock::new(base, size))
    }
    fn take_block(&mut self, base: usize) -> Option<Memblock> {
        let idx = self.data
        .binary_search_by_key(&base, |b| {
            if b.is_empty() { usize::MAX } else { b.base }
        }).ok()?;

        let ret = self.data[idx];
        self.data[idx] = Memblock::empty();
        self.data[idx .. self.len].rotate_left(1);
        self.len -= 1;

        Some(ret)
    }
} 

struct Inner {
    free_blocks: Memblocks,
    reserved_blocks: Memblocks,
}
pub struct BootMemoryManagerBuilder(BootMemoryManager);
impl<'boot> BootMemoryManagerBuilder {
    pub fn new() -> Option<BootMemoryManagerBuilder> {
        static FREE_BLOCKS: SyncUnsafeCell<[Memblock; BootMemoryManager::MAX_MEMBLOCKS_LEN]> = 
            SyncUnsafeCell::new([Memblock::empty(); BootMemoryManager::MAX_MEMBLOCKS_LEN]);
        static RESERVED_BLOCKS: SyncUnsafeCell<[Memblock; BootMemoryManager::MAX_MEMBLOCKS_LEN]> = 
            SyncUnsafeCell::new([Memblock::empty(); BootMemoryManager::MAX_MEMBLOCKS_LEN]);
        static IS_INIT: spin::Mutex<bool> = spin::Mutex::new(true);

        let mut is_init = IS_INIT.lock();
        if !*is_init { return None; }
        
        let free_blocks: Memblocks;
        let reserved_blocks: Memblocks;

        unsafe {
            free_blocks = Memblocks::new(
                FREE_BLOCKS.get().as_mut_unchecked().as_mut_slice()); 
            reserved_blocks = Memblocks::new(
                RESERVED_BLOCKS.get().as_mut_unchecked().as_mut_slice());
        }

        let mba = BootMemoryManager(spin::Mutex::new(
            Inner { free_blocks, reserved_blocks })
        );
        let ret = BootMemoryManagerBuilder(mba);

        *is_init = false;
        Some(ret)
    }

    pub fn add_free(&self, base: usize, size: usize) -> bool {
        self.0.0.lock().free_blocks.put_block(Memblock::new(base, size))
    }
    pub fn add_reserved(&self, base: usize, size: usize) -> bool {
        self.0.0.lock().reserved_blocks.put_block(Memblock::new(base, size))
    }
    pub fn build(self) -> BootMemoryManager {
        self.0
    }
}
  
type PAddr = Addr<LinearSpace>;
pub struct BootMemoryManager(spin::Mutex<Inner>);
impl BootMemoryManager {
    const MAX_MEMBLOCKS_LEN: usize = 128;
    
    pub fn allocate(&self, layout: Layout) -> Option<PAddr> {
        let mut inner_ref = self.0.lock();

        let free_block = inner_ref.free_blocks.cut_block(layout, None)?;
        let ret = free_block.base;
        inner_ref.reserved_blocks.put_block(free_block).then_some(())
            .expect("A block taken from free_blocks should be valid in reserved_blocks");

        Some(PAddr::new(ret))
    }

    pub fn allocate_at(&self, layout: Layout, at: usize) -> Option<PAddr> {
        let mut inner_ref = self.0.lock();

        let free_block = inner_ref.free_blocks.cut_block(layout, Some(at))?;
        let ret = free_block.base;
        inner_ref.reserved_blocks.put_block(free_block).then_some(())
            .expect("A block taken from free_blocks should be valid in reserved_blocks");

        Some(PAddr::new(ret))
    }

    /// Deallocate allocation at `addr`
    /// 
    /// # Safety
    /// - the allocation at `addr` is currently allocated.
    pub unsafe fn deallocate(&self, addr: PAddr) {
        let mut inner_ref = self.0.lock();

        let freed_block = inner_ref.reserved_blocks.take_block(addr.usize())
            .expect("BootMemoryManager should deallocate a currently allocated block");
        inner_ref.free_blocks.put_block(freed_block).then_some(())
            .expect("A block taken from reserved_blocks should be valid in free_blocks");
    }
}