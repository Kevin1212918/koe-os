use alloc::alloc::{AllocError, Allocator};
use core::alloc::Layout;
use core::cell::SyncUnsafeCell;
use core::fmt::Write as _;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::NonNull;
use core::sync::atomic::AtomicUsize;

use arrayvec::ArrayVec;
use derive_more::derive::IntoIterator;
use multiboot2::{MemoryArea, MemoryAreaType};

use crate::mem::addr::{Addr, AddrRange, AddrSpace, PageAddr, PageManager, PageRange, PageSize};
use crate::mem::paging::MemoryManager;
use crate::mem::virt::PhysicalRemapSpace;
use crate::mem::{kernel_end_lma, UMASpace};

pub fn init(memory_areas: &[MemoryArea]) -> Pin<&mut MemblockSystem> {
    // SAFETY: BMM is not accessed elsewhere in the module, and init is called
    // only once.
    let bmm = unsafe { BMM.get().as_mut_unchecked() };
    MemblockSystem::init_pinned(Pin::static_mut(bmm), memory_areas)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemTyp {
    Free,
    Reserved,
}

/// An extent of memory used in `BootMemoryManager``. Note present `Memblock`
/// is considered greater than not present `Memblock`
#[derive(Debug, Clone, Copy)]
pub struct Memblock {
    pub base: Addr<UMASpace>,
    pub size: usize,
    typ: MemTyp,
}
impl Memblock {
    /// Returns an iterator of power-of-2 aligned memblocks, whose order is
    /// in between `min_order` and `max_order`, inclusive.
    pub fn aligned_split(mut self, min_order: u8, max_order: u8) -> AlignedSplit {
        'success: {
            let min_align = 1 << min_order;

            let Some(base) = self.base.align_ceil(min_align) else {
                break 'success;
            };

            self.base = base;

            let Some(end) = (self.base + self.size).align_floor(min_align) else {
                break 'success;
            };

            self.size = match (end - base).try_into() {
                Ok(x) => x,
                Err(_) => break 'success,
            };

            return AlignedSplit {
                memblock: self,
                offset: 0,
                max_order: max_order as u32,
            };
        }

        // Returning an empty iterator.
        AlignedSplit {
            memblock: self,
            offset: self.size,
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
    const fn new() -> Self {
        Self {
            data: ArrayVec::new_const(),
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

        // We use wrapping_sub so that when pivot is 0, it is safely wrapped
        // to usize::MAX, which should not be a valid idx anyways.
        if let Some(prev) = self.data.get(pivot.wrapping_sub(1)) {
            debug_assert!(prev.base <= block.base);
            if block.base.addr_sub(prev.base) as usize == prev.size {
                merge_prev = Some(pivot - 1)
            }
        }

        if let Some(next) = self.data.get(pivot) {
            debug_assert!(next.base >= block.base);
            if next.base.addr_sub(block.base) as usize == block.size {
                merge_next = Some(pivot);
            }
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

    fn remove(&mut self, base: Addr<UMASpace>) -> Option<Memblock> {
        let idx = self.data.binary_search_by_key(&base, |b| b.base).ok()?;

        Some(self.data.remove(idx))
    }

    fn pop(&mut self) -> Option<Memblock> { self.data.pop() }

    fn iter(&self) -> impl Iterator<Item = &Memblock> { self.data.iter() }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Memblock> { self.data.iter_mut() }
}

static BMM: SyncUnsafeCell<MaybeUninit<MemblockSystem>> =
    SyncUnsafeCell::new(MaybeUninit::uninit());
pub struct MemblockSystem {
    free_blocks: Memblocks,
    reserved_blocks: Memblocks,
    partial_block: Option<Memblock>,
    offset: usize,
    managed_range: AddrRange<UMASpace>,
}
impl MemblockSystem {
    pub fn init_pinned<'s, 'm, T>(
        mut slot: Pin<&'s mut MaybeUninit<MemblockSystem>>,
        memory: &'m [T],
    ) -> Pin<&'s mut MemblockSystem>
    where
        Memblock: for<'a> From<&'a T>,
    {
        let tbi = slot.as_mut_ptr();
        // SAFETY: Initializing free_blocks
        unsafe { (&raw mut ((*tbi).free_blocks)).write(Memblocks::new()) };
        // SAFETY: Initializing reserved_blocks
        unsafe { (&raw mut ((*tbi).reserved_blocks)).write(Memblocks::new()) };

        let mut min_addr: Addr<UMASpace> = Addr::new(UMASpace::RANGE.end - 1);
        let mut max_addr: Addr<UMASpace> = Addr::new(UMASpace::RANGE.start);

        for mut block in memory.iter().map(|x| Memblock::from(x)) {
            // Skip the block if it is reserved.
            if block.typ == MemTyp::Reserved {
                continue;
            }
            // Skip the block if it is below end of kernel
            if block.base + block.size < kernel_end_lma() {
                continue;
            }

            if block.base < kernel_end_lma() {
                block.size = block.size - ((kernel_end_lma() - block.base) as usize);
                block.base = kernel_end_lma();
            }

            if block.size == 0 {
                continue;
            }

            min_addr = min_addr.min(block.base);
            max_addr = max_addr.max(block.base + block.size);

            let blocks_ptr = match block.typ {
                // SAFETY: deref in place expression is safe..
                MemTyp::Free => unsafe { &raw mut (*tbi).free_blocks },
                MemTyp::Reserved => unsafe { &raw mut (*tbi).reserved_blocks },
            };
            // SAFETY: blocks_ptr was initialized at the beginning of this
            // function.
            unsafe { blocks_ptr.as_mut_unchecked().insert(block) };
        }

        debug_assert!(
            min_addr < max_addr,
            "No available memory"
        );

        // SAFETY: free_blocks was initalized at the beginning of this function.
        let partial_block = unsafe { (&raw mut (*tbi).free_blocks).as_mut_unchecked().pop() };
        let offset = 0;
        let managed_range = AddrRange {
            base: min_addr,
            size: (max_addr - min_addr) as usize,
        };

        // SAFETY: Initializing partial_block
        unsafe { (&raw mut ((*tbi).partial_block)).write(partial_block) };
        // SAFETY: Initializing offset
        unsafe { (&raw mut ((*tbi).offset)).write(offset) };
        // SAFETY: Initializing managed_range
        unsafe { (&raw mut ((*tbi).managed_range)).write(managed_range) };

        // SAFETY: slot.map_unchecked_mut returns reference to an union variant
        // of the pinned value. tbi is initialized from above.
        unsafe { slot.map_unchecked_mut(|tbi| tbi.assume_init_mut()) }
    }

    pub fn reserve(&mut self, layout: Layout) -> Option<Addr<UMASpace>> {
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

    /// Split the partial block into a free and a reserved block and insert
    /// into the respective [`Memblocks`]. The `MemblockSystem` should not
    /// be modified after.
    pub fn freeze(&mut self) {
        let Some(partial_block) = self.partial_block.take() else {
            return;
        };
        if self.offset == 0 {
            self.free_blocks.insert(partial_block);
        }

        // cut partial block to reserved and free
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

    pub fn managed_range(&self) -> AddrRange<UMASpace> { self.managed_range.clone() }

    pub fn free_blocks(&self) -> &Memblocks { &self.free_blocks }

    pub fn reserved_blocks(&self) -> &Memblocks { &self.reserved_blocks }
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
        if self.offset >= self.memblock.size {
            return None;
        }
        let offset_order = self.offset.trailing_zeros();
        let diff = self.memblock.size - self.offset;
        let diff_order = usize::BITS - diff.leading_zeros() - 1;

        let next_order = offset_order.min(diff_order).min(self.max_order);

        let next_size = 1 << next_order;
        let next = Memblock {
            base: self.memblock.base + self.offset,
            size: next_size,
            typ: self.memblock.typ,
        };
        self.offset += next_size;
        Some(next)
    }
}



//------------------- arch ------------------------

impl PageManager<UMASpace> for MemblockSystem {
    fn allocate_pages(&mut self, cnt: usize, page_size: PageSize) -> Option<PageRange<UMASpace>> {
        let layout = Layout::from_size_align(
            cnt * page_size.usize(),
            page_size.align(),
        )
        .expect("Layout for a page range should be valid");
        let addr = self.reserve(layout)?;
        Some(PageRange {
            base: PageAddr::new(addr, page_size),
            len: cnt,
        })
    }

    unsafe fn deallocate_pages(&mut self, _pages: PageRange<UMASpace>) {
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

    pub fn managed_range(&self) -> AddrRange<UMASpace> {
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
