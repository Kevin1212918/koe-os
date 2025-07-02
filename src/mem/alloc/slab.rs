use alloc::alloc::{AllocError, Allocator};
use alloc::boxed::Box;
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::marker::PhantomData;
use core::mem::{offset_of, transmute, MaybeUninit};
use core::ptr::NonNull;
use core::{array, slice};

use bitvec::order::Lsb0;
use bitvec::slice::BitSlice;
use bitvec::view::BitView;
use pinned_init::{init, init_from_closure, pin_data, Init};

use super::page::PageAllocator;
use super::{allocate_if_zst, deallocate_if_zst};
use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::{self, LinkedList};
use crate::mem::addr::PageSize;

#[derive(Debug, Clone, Copy, Default)]
pub struct SlabAllocator;
// SAFETY: caller uphold allocator guarentees.
unsafe impl Allocator for SlabAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        SLAB_ALLOCATOR_RECORD.allocate(layout)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        // SAFETY: caller uphold allocator guarentees.
        unsafe { SLAB_ALLOCATOR_RECORD.deallocate(ptr, layout) }
    }
}
impl SlabAllocator {
    pub const MAX_ORDER: u8 = 10;
    pub const MAX_SIZE: usize = 1 << Self::MAX_ORDER as usize;
    pub const MIN_ORDER: u8 = 3;
}
static SLAB_ALLOCATOR_RECORD: spin::Lazy<SlabAllocatorRecord> =
    spin::Lazy::new(|| SlabAllocatorRecord {
        caches: array::from_fn(|_| spin::Mutex::new(UntypedCache::new())),
    });

struct SlabAllocatorRecord {
    caches: [spin::Mutex<UntypedCache>; Self::CACHES_CNT],
}
impl SlabAllocatorRecord {}
unsafe impl Allocator for SlabAllocatorRecord {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some(ptr) = allocate_if_zst(layout) {
            return Ok(ptr);
        }

        let slot_order = layout
            .pad_to_align()
            .size()
            .next_multiple_of(1 << SlabAllocator::MIN_ORDER)
            .next_power_of_two()
            .ilog2() as u8;
        let mut cache = self.caches[(slot_order - SlabAllocator::MIN_ORDER) as usize].lock();

        // TODO: Refactor this shit
        // SAFETY: Cache for order i is always located at index i
        unsafe {
            match slot_order {
                0..SlabAllocator::MIN_ORDER => unreachable!(),
                3 => cache.typed::<[u8; 8]>().reserve_untyped(),
                4 => cache.typed::<[u8; 16]>().reserve_untyped(),
                5 => cache.typed::<[u8; 32]>().reserve_untyped(),
                6 => cache.typed::<[u8; 64]>().reserve_untyped(),
                7 => cache.typed::<[u8; 128]>().reserve_untyped(),
                8 => cache.typed::<[u8; 256]>().reserve_untyped(),
                9 => cache.typed::<[u8; 512]>().reserve_untyped(),
                10 => cache.typed::<[u8; 1024]>().reserve_untyped(),

                _ => return Err(AllocError),
            }
        }
        .ok_or(AllocError)
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if deallocate_if_zst(ptr, layout) {
            return;
        }
        let slot_order = layout
            .pad_to_align()
            .size()
            .next_multiple_of(1 << SlabAllocator::MIN_ORDER)
            .next_power_of_two()
            .ilog2() as u8;
        let mut cache = self.caches[(slot_order - SlabAllocator::MIN_ORDER) as usize].lock();
        // TODO: Refactor this shit
        // SAFETY: Cache for order i is always located at index i
        unsafe {
            match slot_order {
                0..SlabAllocator::MIN_ORDER => unreachable!(),
                3 => cache.typed::<[u8; 8]>().free(ptr.cast()),
                4 => cache.typed::<[u8; 16]>().free(ptr.cast()),
                5 => cache.typed::<[u8; 32]>().free(ptr.cast()),
                6 => cache.typed::<[u8; 64]>().free(ptr.cast()),
                7 => cache.typed::<[u8; 128]>().free(ptr.cast()),
                8 => cache.typed::<[u8; 256]>().free(ptr.cast()),
                9 => cache.typed::<[u8; 512]>().free(ptr.cast()),
                10 => cache.typed::<[u8; 1024]>().free(ptr.cast()),

                _ => panic!("Free unallocated"),
            }
        }
    }
}

impl SlabAllocatorRecord {
    const CACHES_CNT: usize = (SlabAllocator::MAX_ORDER - SlabAllocator::MIN_ORDER + 1) as usize;
}
impl<const N: usize> Item for [u8; N] {
    const LAYOUT: Layout = {
        assert!(N != 0);
        assert!(N.is_power_of_two());
        // SAFETY: Checked from the assertions above
        unsafe { Layout::from_size_align_unchecked(N, N) }
    };
}


/// A type-erased [`Cache`].
pub struct UntypedCache {
    empty_slabs: LinkedList<SLAB_LINK_OFFSET, BoxSlab>,
    partial_slabs: LinkedList<SLAB_LINK_OFFSET, BoxSlab>,
    full_slabs: LinkedList<SLAB_LINK_OFFSET, BoxSlab>,
}
impl UntypedCache {
    /// Creates a type-erased cache.
    ///
    /// Call [`Self::typed`] to assign a type to the cache and expose the
    /// typed interface.
    fn new() -> Self {
        Self {
            empty_slabs: LinkedList::<SLAB_LINK_OFFSET, BoxSlab>::new_in(PageAllocator),
            partial_slabs: LinkedList::<SLAB_LINK_OFFSET, BoxSlab>::new_in(PageAllocator),
            full_slabs: LinkedList::<SLAB_LINK_OFFSET, BoxSlab>::new_in(PageAllocator),
        }
    }

    /// Get a typed view into the `self`
    ///
    /// # Safety
    /// `T` is the underlying type.
    unsafe fn typed<T: Item>(&mut self) -> &mut Cache<T> {
        debug_assert!(Layout::new::<Cache<[u8; 3]>>() == Layout::new::<UntypedCache>());
        // SAFETY: Cache<T> has same layout as UntypedCache, caller guarentees the
        // transmuted type is correct.
        unsafe { transmute(self) }
    }
}

/// A slab allocator for `T`.
#[repr(transparent)]
pub struct Cache<T: Item> {
    inner: UntypedCache,
    _phantom: PhantomData<T>,
}

type BoxSlab = Box<UnsafeCell<UntypedSlab>, PageAllocator>;
impl<T: Item> Cache<T> {
    fn new() -> Self {
        Self {
            inner: UntypedCache::new(),
            _phantom: PhantomData,
        }
    }

    fn reserve_untyped(&mut self) -> Option<NonNull<[u8]>> {
        let ptr = self.reserve()?;
        Some(NonNull::slice_from_raw_parts(
            ptr.cast(),
            T::LAYOUT.size(),
        ))
    }

    fn reserve(&mut self) -> Option<NonNull<T>> {
        // Find a non-full slab by first checking partial, then empty, then creating new
        // slab.
        let mut slab_cursor = self.inner.partial_slabs.front_mut();

        if slab_cursor.is_null() {
            slab_cursor = self.inner.empty_slabs.front_mut();
        }
        if slab_cursor.is_null() {
            let mut box_slab = BoxSlab::try_new_uninit_in(PageAllocator).ok()?;
            // SAFETY: BoxSlab holds a UnsafeCell<UntypedSlab>, which has the same layout as
            // Slab<T>. On initialization failure, function will early exit and
            // deallocate.
            unsafe { Slab::<T>::new_unsafe_cell().__init(box_slab.as_mut_ptr().cast()) }.ok()?;
            // SAFETY: Initialized above.
            slab_cursor.insert_after(unsafe { box_slab.assume_init() });
            slab_cursor.move_next();
        }

        debug_assert!(
            !slab_cursor.is_null(),
            "Cursor should now point to a valid slab"
        );

        // SAFETY: Cursor is not null.
        let slab = unsafe { slab_cursor.get().unwrap_unchecked() };
        // SAFETY: Cache exclusively owns the slab. Since there is a mutable
        // reference to cache, there cannot be other references to the slab.
        let slab = unsafe { slab.get().as_mut_unchecked() };
        // SAFETY: All slabs on a cache has the was created on the cache with the type T
        let slab: &mut Slab<T> = unsafe { slab.typed() };


        let was_empty = slab.is_empty();
        let ret = slab.reserve();
        let is_full = slab.is_full();

        if !was_empty && !is_full {
            return ret;
        }

        // SAFETY: slab_cursor points to a slab.
        let slab = unsafe { slab_cursor.remove().unwrap_unchecked() };
        let list = if is_full {
            &mut self.inner.full_slabs
        } else {
            &mut self.inner.partial_slabs
        };
        list.push_front(slab);
        ret
    }

    unsafe fn free(&mut self, ptr: NonNull<T>) {
        // SAFETY: ptr was reserved from one of the slabs.
        let mut slab_ptr = unsafe { Slab::from_elem_ptr(ptr) };
        // SAFETY: Cache exclusively owns the slab. Since there is a mutable
        // reference to cache, there cannot be other references to the slab.
        let slab = unsafe { slab_ptr.as_mut() };


        let was_full = slab.is_full();
        // SAFETY: ptr was reserved from this slab.
        unsafe { slab.free(ptr) };
        let is_empty = slab.is_empty();

        if !is_empty && !was_full {
            return;
        }

        // Fixing the slab lists.
        let list = if was_full {
            &mut self.inner.full_slabs
        } else {
            &mut self.inner.partial_slabs
        };
        // SAFETY: if the slab was full, then it is in full_slabs, otherwise
        // it is in partial slabs.
        let mut cursor = unsafe { list.cursor_mut_from_ptr(slab_ptr.as_ptr().cast_const().cast()) };
        // SAFETY: cursor is created from a ptr to element.
        let slab = unsafe { cursor.remove().unwrap_unchecked() };
        let list = if is_empty {
            &mut self.inner.empty_slabs
        } else {
            &mut self.inner.partial_slabs
        };
        list.push_front(slab);
    }
}
#[derive(PartialEq, Eq)]
enum SlabFillLevel {
    Empty,
    Partial,
    Full,
}

// TODO: Use atomic map to allow concurrent modification.

/// Size of a `Slab` page
const SLAB_PAGE: PageSize = PageSize::Small;
/// The length of slab map array in terms of the number of u64s. Multiply by 64
/// for the number of bits in the bitmap.
const SLAB_MAP_LEN: usize = 8;
/// Size of the available memory for slots array.
const SLAB_BUF_SIZE: usize = SLAB_PAGE.usize()
    - size_of::<spin::Mutex<()>>()
    - size_of::<u16>()
    - size_of::<ll::Link>()
    - SLAB_MAP_LEN * size_of::<usize>();

// TODO: figure out a way to compile time ensure slab alignment.

/// A page-sized slab with metadata.
///
/// Fits into a [`SLAB_PAGE`] and **must** be `SLAB_PAGE` aligned.
#[pin_data]
#[repr(transparent)]
struct Slab<T: Item> {
    #[pin]
    inner: UntypedSlab,
    _phantom: PhantomData<T>,
}

// TODO: workaround
// Annoying hack to ensure layout of Slab is generic.
#[repr(C)]
struct UntypedSlab {
    link: ll::Link,
    /// Bitmap for the slot array. 0 is occupied and 1 is free.
    bitmap: [u64; SLAB_MAP_LEN],
    free_cnt: u16,
    buf: [u8; SLAB_BUF_SIZE],
}
impl UntypedSlab {
    fn new(free_cnt: u16) -> impl Init<Self> {
        init!(Self {
            link: ll::Link::new(),
            bitmap <- pinned_init::zeroed(),
            free_cnt,
            buf <- pinned_init::zeroed(),
        })
        .chain(|slab| {
            slab.bitmap.fill(u64::MAX);
            Ok(())
        })
    }

    unsafe fn typed<T: Item>(&mut self) -> &mut Slab<T> {
        // SAFETY: UntypedSlab has same type layout as Slab.
        unsafe { transmute(self) }
    }
}

const SLAB_LINK_OFFSET: usize = offset_of!(UntypedSlab, link);
unsafe impl ll::Linked<SLAB_LINK_OFFSET> for UnsafeCell<UntypedSlab> {}

/// A slab consisting of a link, a bitmap, some padding, and an array of managed
/// slots.
///
/// `buf: | padding | slots |`
impl<T: Item> Slab<T> {
    /// Number of slots in a `Slab`
    const SLOTS_LEN: usize = {
        let residual_size = SLAB_BUF_SIZE - Self::SLOTS_START;
        residual_size / Self::SLOT_SIZE
    };
    /// Offset into `Slab.buf` where the slots array start.
    const SLOTS_START: usize = const {
        let buf_start = offset_of!(UntypedSlab, buf);
        let slots_start = buf_start.next_multiple_of(Self::SLOT_ALIGN) - buf_start;
        assert!(slots_start < SLAB_PAGE.usize());
        slots_start
    };
    /// Align of a slot in bytes.
    const SLOT_ALIGN: usize = { T::LAYOUT.align() };
    /// Size of a slot in bytes.
    const SLOT_SIZE: usize = { T::LAYOUT.pad_to_align().size() };

    const _ASSERT_SLOT_LEN_IS_AT_LEAST_TWO: () = assert!(Self::SLOTS_LEN >= 2);
    const _ASSERT_SLOT_SIZE_DOES_NOT_OVERFLOW: () = {
        let buf_start = offset_of!(UntypedSlab, buf);
        assert!(buf_start + Self::SLOTS_START + Self::SLOT_SIZE <= SLAB_PAGE.usize());
    };

    fn map(&self) -> &BitSlice<u64, Lsb0> { &self.inner.bitmap.view_bits()[0..Self::SLOTS_LEN] }

    fn map_mut(&mut self) -> &mut BitSlice<u64, Lsb0> {
        &mut self.inner.bitmap.view_bits_mut()[0..Self::SLOTS_LEN]
    }

    fn slots(&self) -> &[MaybeUninit<T>] {
        // SAFETY: Self::SLOTS_START is smaller than page size, so the ptr is
        // valid.
        let slots_start = unsafe { (&raw const self.inner.buf).byte_add(Self::SLOTS_START) };
        let slots_start = slots_start.cast::<MaybeUninit<T>>();
        // SAFETY: We have immutable reference over the slab from the function
        // parameter. &raw buf + SLOTS_START + SLOT_SIZE does not overflow.
        unsafe { slice::from_raw_parts(slots_start, Self::SLOT_SIZE) }
    }

    fn slots_mut(&mut self) -> &mut [MaybeUninit<T>] {
        // SAFETY: Self::SLOTS_START is smaller than page size, so the ptr is
        // valid.
        let slots_start = unsafe { (&raw mut self.inner.buf).byte_add(Self::SLOTS_START) };
        let slots_start = slots_start.cast::<MaybeUninit<T>>();
        // SAFETY: We have immutable reference over the slab from the function
        // parameter. &raw buf + SLOTS_START + SLOT_SIZE does not overflow.
        unsafe { slice::from_raw_parts_mut(slots_start, Self::SLOT_SIZE) }
    }

    fn is_empty(&self) -> bool { matches!(self.fill_level(), SlabFillLevel::Empty) }

    fn is_full(&self) -> bool { matches!(self.fill_level(), SlabFillLevel::Full) }

    fn fill_level(&self) -> SlabFillLevel {
        match self.inner.free_cnt as usize {
            0 => SlabFillLevel::Empty,
            slots_len => SlabFillLevel::Full,
            _ => SlabFillLevel::Partial,
        }
    }

    fn new() -> impl Init<Self> {
        init!(Self {
            inner <- UntypedSlab::new(Self::SLOTS_LEN as u16),
            _phantom: PhantomData
        })
    }

    fn new_unsafe_cell() -> impl Init<UnsafeCell<Self>> {
        // SAFETY: Initializing a pointer to unsafe cell using the element's
        // in place intializer should be safe.
        unsafe {
            init_from_closure(|slot: *mut UnsafeCell<Self>| Self::new().__init(slot.cast::<Self>()))
        }
    }

    fn reserve(&mut self) -> Option<NonNull<T>> {
        if self.inner.free_cnt as usize == 0 {
            return None;
        }

        let map = self.map_mut();
        // SAFETY: Since free_cnt is not greater or equal to SLOTS_LEN, there
        // must be at least one slot not occupied.
        // let idx = unsafe { map.first_one().unwrap_unchecked() };
        let idx = map.first_one().unwrap();
        debug_assert!(idx < Self::SLOTS_LEN);
        // SAFETY: idx was returned from first_one
        unsafe { map.replace_unchecked(idx, false) };

        self.inner.free_cnt -= 1;
        let uninit = &mut self.slots_mut()[idx];
        NonNull::new(uninit.as_mut_ptr().cast())
    }

    unsafe fn free(&mut self, ptr: NonNull<T>) {
        debug_assert!(self.inner.free_cnt > 0);
        debug_assert!(self
            .inner
            .buf
            .as_mut_ptr_range()
            .contains(&ptr.as_ptr().cast()));

        // SAFETY: The ptr was reserved from this slab as guarenteed by caller.
        let idx = unsafe { ptr.as_ptr().offset_from(self.slots().as_ptr().cast()) };
        let idx = idx as usize;

        debug_assert!(!self.map()[idx]);
        // SAFETY: Since ptr is within bound, its offset from beginning of
        // slots should be within bound as well.
        unsafe { self.map_mut().replace_unchecked(idx, true) };
        self.inner.free_cnt += 1;
    }

    /// Derive a pointer to the containing slab from a pointer to an element.
    ///
    /// # Safety
    /// 'ptr' points to an element in an existing slab.
    unsafe fn from_elem_ptr(ptr: NonNull<T>) -> NonNull<Self> {
        // FIXME: FIX THIS
        let offset = (ptr.as_ptr() as usize) % SLAB_PAGE.align();
        // SAFETY: No slab should be at 0 pointer.
        unsafe { ptr.byte_sub(offset) }.cast()
    }
}

pub trait Item: Sized {
    const LAYOUT: Layout = Layout::new::<Self>();
    const _ASSERT_ITEM_IS_ALIGNED_TO_SLAB_PAGE: () =
        assert!(Self::LAYOUT.align() <= SLAB_PAGE.align());
}
