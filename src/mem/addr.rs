use core::alloc::Layout;
use core::marker::PhantomData;
use core::ops::{Add, Deref, DerefMut, Range, Sub};

use derive_more::derive::Into;
use strum::VariantArray;

use super::virt::VirtSpace;
use crate::common::{GiB, KiB, MiB};

/// An address space with constant evaluated address bounds. All addresses are
/// within an address space, and addresses derived from address operations will
/// remain within the address space.
///
/// The address space must be [`PageSize::MAX`] aligned.
pub trait AddrSpace: Clone + Copy + PartialEq + Eq + PartialOrd + Ord {
    /// The range of valid addresses.
    const RANGE: Range<usize>;
    /// Unit const for assertion.
    const _ASSERT_RANGE_IS_PAGE_ALIGNED: () = assert_range_is_page_aligned::<Self>();

    const MIN_ADDR: Addr<Self> = { Addr::new(Self::RANGE.start) };
    const MAX_ADDR: Addr<Self> = { Addr::new(Self::RANGE.end - 1) };
}
const fn assert_range_is_page_aligned<S: AddrSpace>() {
    assert!(S::RANGE.start % PageSize::MAX.align() == 0);
    assert!(S::RANGE.end % PageSize::MAX.align() == 0);
}

/// An address within the [`AddrSpace`].
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Addr<S: AddrSpace> {
    value: usize,
    _addr_space: PhantomData<S>,
}
impl<S: AddrSpace> Add<usize> for Addr<S> {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output { self.byte_add(rhs) }
}

impl<S: AddrSpace> Sub<usize> for Addr<S> {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output { self.byte_sub(rhs) }
}

impl<S: AddrSpace> Sub for Addr<S> {
    type Output = isize;

    fn sub(self, rhs: Self) -> Self::Output { self.addr_sub(rhs) }
}

impl<S: AddrSpace> Addr<S> {
    /// Creates a new address in the address space from `value`.
    ///
    /// # Undefined Behavior
    /// The resulting address should be within the address space.
    pub const fn new(value: usize) -> Self {
        debug_assert!(S::RANGE.start <= value && value < S::RANGE.end);

        Self {
            value,
            _addr_space: PhantomData,
        }
    }

    /// Converts the address into a `usize`.
    pub const fn usize(self) -> usize { self.value }

    /// Add `x` bytes to the address.
    ///
    /// # Undefined Behavior
    /// The resulting address should not overflow the address space.
    pub const fn byte_add(mut self, x: usize) -> Self {
        debug_assert!(S::RANGE.start <= self.value + x);

        self.value += x;
        self
    }

    /// Add `x` bytes to the address. Returns `None` if the resulting address
    /// overflows the address space.
    pub fn checked_byte_add(mut self, x: usize) -> Option<Self> {
        self.value
            .checked_add(x)
            .filter(|x| S::RANGE.contains(x))
            .map(|x| {
                self.value = x;
                self
            })
    }

    /// Subtract `x` bytes to the address.
    ///
    /// # Undefined Behavior
    /// The resulting address should not underflow the address space.
    pub const fn byte_sub(mut self, x: usize) -> Self {
        debug_assert!(self.value - x < S::RANGE.end);

        self.value -= x;
        self
    }

    /// Subtract `x` bytes to the address. Returns `None` if the resulting
    /// address underflows the address space.
    pub fn checked_byte_sub(mut self, x: usize) -> Option<Self> {
        self.value
            .checked_sub(x)
            .filter(|x| S::RANGE.contains(x))
            .map(|x| {
                self.value = x;
                self
            })
    }

    /// Returns the signed byte difference between two addresses.
    pub const fn addr_sub(self, x: Self) -> isize { self.value as isize - x.value as isize }

    /// Returns an aligned address by rounding above. None if no aligned address
    /// is above and within the address space.
    ///
    /// # Panics
    /// align is 0.
    pub fn align_ceil(mut self, align: usize) -> Option<Self> {
        let start = S::RANGE.start;
        let end = S::RANGE.end;
        self.usize()
            .checked_next_multiple_of(align)
            .filter(|&x| x < end)
            .map(|x| {
                self.value = x;
                self
            })
    }

    /// Returns an aligned address by rounding below. None if no aligned address
    /// is below and within the address space.
    ///
    /// # Panics
    /// align is 0.
    pub fn align_floor(mut self, align: usize) -> Option<Self> {
        let start = S::RANGE.start;
        let end = S::RANGE.end;
        let val = self.usize();
        Some(val - val % align).filter(|&x| x >= start).map(|x| {
            self.value = x;
            self
        })
    }

    pub const fn is_aligned_to(self, alignment: usize) -> bool { self.value % alignment == 0 }
}

impl<S: VirtSpace> Addr<S> {
    pub fn from_ref<T>(value: &T) -> Self { Self::from_ptr(value as *const T) }

    pub fn from_mut<T>(value: &mut T) -> Self { Self::from_mut_ptr(value as *mut T) }

    pub fn from_ptr<T>(value: *const T) -> Self { Addr::new(value as usize) }

    pub fn from_mut_ptr<T>(value: *mut T) -> Self { Addr::new(value as usize) }

    pub fn into_ptr<T>(self) -> *mut T { self.usize() as *mut T }

    pub fn index_range(self, range: &Range<usize>) -> usize {
        if range.is_empty() {
            return 0;
        }
        let mask = 1usize.strict_shl(range.end as u32) - 1;
        (self.value & mask) >> range.start
    }
}

#[derive(Debug, Clone, Copy)]
/// An address range in the [`AddrSpace`].
pub struct AddrRange<S: AddrSpace> {
    pub base: Addr<S>,
    pub size: usize,
}
impl<S: AddrSpace> From<Range<Addr<S>>> for AddrRange<S> {
    fn from(value: Range<Addr<S>>) -> Self {
        let base = value.start;
        let size = (value.end - value.start).try_into().unwrap_or(0);
        Self { base, size }
    }
}
impl<S: AddrSpace> AddrRange<S> {
    /// Creates a new address range from base address and range size.
    ///
    /// # Undefined Behavior
    /// The address range should be fully contained within the address space.
    pub const fn new(base: Addr<S>, size: usize) -> Self {
        debug_assert!(S::RANGE.end >= base.usize() + size);
        Self { base, size }
    }

    /// Returns start of the address range.
    pub const fn start(&self) -> Addr<S> { self.base }

    /// Returns end of the address range.
    pub const fn end(&self) -> Addr<S> { self.base.byte_add(self.size) }

    /// Check if the address range is empty.
    pub const fn is_empty(&self) -> bool { self.size == 0 }

    /// Returns an empty range.
    pub const fn empty() -> Self {
        Self {
            base: Addr::new(S::RANGE.start),
            size: 0,
        }
    }

    /// Check if ranges overlaps.
    pub const fn overlaps(&self, other: &Self) -> bool {
        // NOTE: Accessing value directly due to const restraint
        self.start().value < other.end().value && other.start().value < self.end().value
    }

    /// Check if this range contains the other range.
    pub const fn contains(&self, other: &Self) -> bool {
        // NOTE: Accessing value directly due to const restraint
        self.start().value <= other.start().value && other.end().value <= self.end().value
    }

    /// Returns the set intersect of 'self' and 'rhs'. If the result is empty,
    /// base of the range is unspecified.
    pub fn range_intersect(&self, rhs: &Self) -> Self {
        let start = self.start().max(rhs.start());
        let end = self.end().min(rhs.end());
        AddrRange::from(start..end)
    }

    /// Returns the set union of 'self' and 'rhs'.
    ///
    /// In order to handle the resulting disjoint ranges, two address ranges
    /// are returned. The returned ranges may be empty with an unspecified base.
    pub fn range_sum(&self, rhs: &Self) -> [Self; 2] {
        if !self.overlaps(rhs) {
            return [self.clone(), rhs.clone()];
        }
        let start = self.start().min(rhs.start());
        let end = self.end().max(rhs.end());
        [AddrRange::from(start..end), AddrRange::empty()]
    }

    /// Returns the set union of 'self' and 'rhs'. None if 'self' and 'rhs' do
    /// not overlap.
    pub fn range_sum_strict(&self, rhs: &Self) -> Option<Self> {
        if !self.overlaps(rhs) {
            return None;
        }
        let start = self.start().min(rhs.start());
        let end = self.end().max(rhs.end());
        Some(AddrRange::from(start..end))
    }


    /// Returns the set subtraction of `rhs` from `self`.
    ///
    /// In order to handle the resulting disjoint ranges, two address ranges
    /// are returned. The returned ranges may be empty with an unspecified base.
    pub fn range_sub(&self, rhs: Self) -> [Self; 2] {
        if self.is_empty() {
            return [Self::empty(), rhs];
        }
        if rhs.is_empty() {
            return [self.clone(), Self::empty()];
        }

        let low_size: usize = (rhs.base - self.base)
            .try_into()
            .unwrap_or(0)
            .min(self.size);
        let hi_size: usize = (self.base - rhs.base + (self.size as isize - rhs.size as isize))
            .try_into()
            .unwrap_or(0)
            .min(self.size);
        let lo = Self::new(self.base, low_size);
        let hi = Self::new(
            self.base + (self.size - hi_size),
            hi_size,
        );
        [lo, hi]
    }

    /// Returns the range of **fully** contained pages.
    pub fn contained_pages(&self, page_size: PageSize) -> PageRange<S> {
        if self.size < page_size.usize() {
            return PageRange::empty(page_size);
        }

        let base = self
            .base
            .align_ceil(page_size.usize())
            .expect("AddrSpace should be page-aligned.");

        let residual = self.size - (base - self.base) as usize;
        PageRange {
            base: Page::new(base, page_size),
            len: residual / page_size.usize(),
        }
    }

    /// Returns the range of overlapped pages.
    pub fn overlapped_pages(&self, page_size: PageSize) -> PageRange<S> {
        let base = self
            .base
            .align_floor(page_size.usize())
            .expect("AddrSpace should be page-aligned.");

        let residual = self.size + (self.base - base) as usize;
        PageRange {
            base: Page::new(base, page_size),
            len: residual.div_ceil(page_size.usize()),
        }
    }

    pub fn split_aligned(mut self, min_order: u8, max_order: u8) -> SplitAligned<S> {
        let empty = SplitAligned {
            range: self,
            offset: self.size,
            max_order: max_order as u32,
        };

        let min_align = 1 << min_order;

        let Some(base) = self.start().align_ceil(min_align) else {
            return empty;
        };

        self.base = base;

        let Some(end) = self.end().align_floor(min_align) else {
            return empty;
        };

        self.size = match (end - base).try_into() {
            Ok(x) => x,
            Err(_) => return empty,
        };

        return SplitAligned {
            range: self,
            offset: 0,
            max_order: max_order as u32,
        };
    }
}

/// An iterator of power-of-2 aligned ranges splitted from a single
/// range.
pub struct SplitAligned<S: AddrSpace> {
    range: AddrRange<S>,
    offset: usize,
    max_order: u32,
}
impl<S: AddrSpace> Iterator for SplitAligned<S> {
    type Item = AddrRange<S>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.range.size {
            return None;
        }
        let offset_order = self.offset.trailing_zeros();
        let diff = self.range.size - self.offset;
        let diff_order = usize::BITS - diff.leading_zeros() - 1;

        let next_order = offset_order.min(diff_order).min(self.max_order);

        let next_size = 1 << next_order;
        let next = AddrRange {
            base: self.range.base + self.offset,
            size: next_size,
        };
        self.offset += next_size;
        Some(next)
    }
}

/// A page aligned address.
#[derive(Debug, Clone, Copy, Into)]
pub struct Page<S: AddrSpace> {
    #[into]
    base: Addr<S>,
    page_size: PageSize,
}
impl<S: AddrSpace> Page<S> {
    /// Creates a new page descriptor for the page at `base` of `size`
    ///
    /// # Panics
    /// panics if `base` is not page aligned
    pub const fn new(base: Addr<S>, page_size: PageSize) -> Self {
        let align: usize = page_size.align();
        assert!(base.usize() % align == 0);
        Self { base, page_size }
    }

    /// Returns the underlying address.
    pub const fn addr(&self) -> Addr<S> { self.base }

    /// Alias of [`Self::addr`]
    pub const fn start(&self) -> Addr<S> { self.base }

    /// Returns the underlying page size.
    pub const fn page_size(&self) -> PageSize { self.page_size }

    /// Increment the address by `page_cnt` pages.
    ///
    /// Returns `None` if the resulting address overflows the address space.
    pub fn checked_page_add(mut self, page_cnt: usize) -> Option<Self> {
        self.base = self
            .base
            .checked_byte_add(page_cnt * self.page_size.usize())?;
        Some(self)
    }

    /// Decrement the address by `page_cnt` pages.
    ///
    /// Returns `None` if the resulting address overflows the address space.
    pub fn checked_page_sub(mut self, page_cnt: usize) -> Option<Self> {
        self.base = self
            .base
            .checked_byte_sub(page_cnt * self.page_size.usize())?;
        Some(self)
    }
}

/// A contiguous range of pages
#[derive(Debug, Clone, Copy)]
pub struct PageRange<S: AddrSpace> {
    /// Starting address of the page range.
    pub base: Page<S>,
    /// Number of pages in the page range.
    pub len: usize,
}
impl<S: AddrSpace> Into<AddrRange<S>> for PageRange<S> {
    fn into(self) -> AddrRange<S> {
        let size = self.len * self.base.page_size().usize();
        let base = self.base.addr();
        AddrRange { base, size }
    }
}
impl<S: AddrSpace> IntoIterator for PageRange<S> {
    type Item = Page<S>;

    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.base.base.usize();
        let step = self.page_size().usize();

        (start..)
            .step_by(step)
            .take(self.len)
            .map(move |base| Page::new(Addr::new(base), self.page_size()))
    }
}
impl<S: AddrSpace> PageRange<S> {
    /// Creates an empty page range aligned to `page_size`.
    pub const fn empty(page_size: PageSize) -> Self {
        let base = Page::new(Addr::new(S::RANGE.start), page_size);
        let len = 0;
        Self { base, len }
    }

    /// Returns the underlying page size.
    pub const fn page_size(&self) -> PageSize { self.base.page_size }

    /// Create a `PageRange` from an address range.
    ///
    /// Returns `None` if the range is not page aligned.
    pub const fn try_from_range(range: AddrRange<S>, page_size: PageSize) -> Option<Self> {
        if !range.base.is_aligned_to(page_size.align()) {
            return None;
        }

        let base = Page::new(range.base, page_size);
        if range.size % page_size.align() != 0 {
            return None;
        }

        let len = range.size / page_size.align();

        Some(Self { base, len })
    }
}

/// An allocator which manages an address space. This trait is based on
/// [`Allocator`][core::alloc::Allocator].
///
/// # Safety
///
/// Blocks that are *currently allocated* by an allocator, must not be allocated
/// by another allocator until either:
/// - the block is deallocated, or
/// - the allocator is dropped.
///
/// Copying, cloning, or moving the allocator must not invalidate blocks
/// returned from it. A copied or cloned allocator must behave like the original
/// allocator.
pub unsafe trait Allocator<S: AddrSpace> {
    /// Attempts to allocate a block. On success, returns an address
    /// range that meet the size and alignment guarentee of layout.
    ///
    /// See [allocate][core::alloc::Allocator::allocate] for more details.
    fn allocate(&self, layout: Layout) -> Option<AddrRange<S>>;

    /// Deallocate the block starting at `addr`.
    ///
    /// See [allocate][core::alloc::Allocator::deallocate] for more details.
    ///
    /// # Safety
    /// - `addr` must denote a block *currently allocated* via this allocator,
    ///   and
    /// - `layout` must fit the block of memory.
    unsafe fn deallocate(&self, addr: Addr<S>, layout: Layout);
}

unsafe impl<S: AddrSpace, A: Allocator<S>> Allocator<S> for &A {
    fn allocate(&self, layout: Layout) -> Option<AddrRange<S>> { (*self).allocate(layout) }

    unsafe fn deallocate(&self, addr: Addr<S>, layout: Layout) {
        unsafe { (*self).deallocate(addr, layout) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Hardware paging page size.
///
/// # Requirements
/// The kernel assumes certain properties regarding the pages.
/// - All page sizes are powers of two.
/// - The alignment equals the size of a page.
/// - Page variants are listed in order.
#[repr(u8)]
#[derive(strum_macros::VariantArray)]
pub enum PageSize {
    Small = 12,
    Large = 21,
    Huge = 30,
}
impl PageSize {
    /// Largest page size
    pub const MAX: Self = Self::Huge;
    /// Smallest page size
    pub const MIN: Self = Self::Small;

    /// Returns memory layout of a page of size `self`.
    pub fn layout(self) -> Layout {
        Layout::from_size_align(self.usize(), self.align())
            .expect("PageSize should specify a valid page layout.")
    }

    /// Returns memory alignment in bytes of a page of size `self`.
    ///
    /// This should be same as the page's size in bytes.
    pub const fn align(self) -> usize { self.usize() }

    /// Returns base-2 log of page size in bytes.
    pub const fn order(self) -> u8 { self as u8 }

    /// Returns page size in bytes.
    pub const fn usize(self) -> usize { 1 << self as u8 as usize }

    /// Returns the size of a fitting page for `layout`. `None` if no single
    /// page can hold the type specified by `layout`.
    pub const fn fit(layout: Layout) -> Option<Self> {
        let layout = layout.pad_to_align();
        let align_order = (usize::BITS - layout.size().leading_zeros()) as u8;

        let mut cur = 0;
        while cur < Self::VARIANTS.len() {
            if align_order <= Self::VARIANTS[cur] as u8 {
                return Some(Self::VARIANTS[cur]);
            }
            cur += 1;
        }
        None
    }
}
impl Into<usize> for PageSize {
    fn into(self) -> usize { self.usize() }
}
