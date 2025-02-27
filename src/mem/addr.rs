use core::iter;
use core::marker::PhantomData;
use core::ops::{Add, Range, RangeBounds, Sub};

use derive_more::derive::Into;

use super::virt::VirtSpace;
use crate::common::{GiB, KiB, MiB};

pub trait AddrSpace: Clone + Copy + PartialEq + Eq + PartialOrd + Ord {
    const RANGE: Range<usize>;
    /// Unit const for assertion.
    const _ASSERT_RANGE_IS_PAGE_ALIGNED: () = assert_range_is_page_aligned::<Self>();
}
const fn assert_range_is_page_aligned<S: AddrSpace>() {
    assert!(S::RANGE.start % PageSize::MAX.align() == 0);
    assert!(S::RANGE.end % PageSize::MAX.align() == 0);
}

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
    pub const fn new(value: usize) -> Self {
        assert!(S::RANGE.start <= value && value < S::RANGE.end);

        Self {
            value,
            _addr_space: PhantomData,
        }
    }

    pub const fn usize(self) -> usize { self.value }

    pub const fn byte_add(mut self, x: usize) -> Self {
        debug_assert!(S::RANGE.start <= self.value + x);

        self.value += x;
        self
    }

    pub fn checked_byte_add(mut self, x: usize) -> Option<Self> {
        self.value
            .checked_add(x)
            .filter(|x| S::RANGE.contains(x))
            .map(|x| {
                self.value = x;
                self
            })
    }

    pub const fn byte_sub(mut self, x: usize) -> Self {
        debug_assert!(self.value - x < S::RANGE.end);

        self.value -= x;
        self
    }

    pub fn checked_byte_sub(mut self, x: usize) -> Option<Self> {
        self.value
            .checked_sub(x)
            .filter(|x| S::RANGE.contains(x))
            .map(|x| {
                self.value = x;
                self
            })
    }

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
    pub fn from_ref<T>(value: &T) -> Self { Addr::new(value as *const T as usize) }

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
    pub const fn new(base: Addr<S>, size: usize) -> Self {
        debug_assert!(S::RANGE.end - (base.usize() + size) >= 0);

        Self { base, size }
    }

    pub const fn start(&self) -> Addr<S> { self.base }

    pub const fn end(&self) -> Addr<S> { self.base.byte_add(self.size) }

    pub const fn is_empty(&self) -> bool { self.size == 0 }

    pub const fn empty() -> Self {
        Self {
            base: Addr::new(S::RANGE.start),
            size: 0,
        }
    }

    /// Returns the set subtraction of `rhs` from `self`.
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
            base: PageAddr::new(base, page_size),
            len: residual / page_size.usize(),
        }
    }

    pub fn overlapped_pages(&self, page_size: PageSize) -> PageRange<S> {
        let base = self
            .base
            .align_floor(page_size.usize())
            .expect("AddrSpace should be page-aligned.");

        let residual = self.size + (self.base - base) as usize;
        PageRange {
            base: PageAddr::new(base, page_size),
            len: residual.div_ceil(page_size.usize()),
        }
    }
}
/// A page consists a page aligned address and a page size
#[derive(Debug, Clone, Copy, Into)]
pub struct PageAddr<S: AddrSpace> {
    #[into]
    base: Addr<S>,
    page_size: PageSize,
}
impl<S: AddrSpace> PageAddr<S> {
    /// Creates a new page descriptor for the page at `base` of `size`
    ///
    /// # Panics
    /// panics if `base` is not page aligned
    pub const fn new(base: Addr<S>, page_size: PageSize) -> Self {
        let align: usize = page_size.align();
        assert!(base.usize() % align == 0);
        Self { base, page_size }
    }

    pub const fn addr(&self) -> Addr<S> { self.base }

    pub const fn start(&self) -> Addr<S> { self.base }

    pub const fn end(&self) -> Addr<S> { self.base.byte_add(self.page_size.usize()) }

    pub const fn page_size(&self) -> PageSize { self.page_size }

    pub const fn size(&self) -> usize { self.page_size.usize() }

    pub const fn range(&self) -> Range<Addr<S>> { self.start()..self.end() }

    pub fn checked_page_add(mut self, page_cnt: usize) -> Option<Self> {
        self.base = self
            .base
            .checked_byte_add(page_cnt * self.page_size.usize())?;
        Some(self)
    }

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
    pub base: PageAddr<S>,
    pub len: usize,
}
impl<S: AddrSpace> Into<AddrRange<S>> for PageRange<S> {
    fn into(self) -> AddrRange<S> {
        let size = self.len * self.base.size();
        let base = self.base.addr();
        AddrRange { base, size }
    }
}
impl<S: AddrSpace> IntoIterator for PageRange<S> {
    type Item = PageAddr<S>;

    type IntoIter = impl Iterator<Item = Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.base.base.usize();
        let step = self.page_size().usize();

        (start..)
            .step_by(step)
            .take(self.len)
            .map(move |base| PageAddr::new(Addr::new(base), self.page_size()))
    }
}
impl<S: AddrSpace> PageRange<S> {
    pub const fn empty(page_size: PageSize) -> Self {
        let base = PageAddr::new(Addr::new(S::RANGE.start), page_size);
        let len = 0;
        Self { base, len }
    }

    pub const fn page_size(&self) -> PageSize { self.base.page_size }

    pub const fn try_from_range(range: AddrRange<S>, page_size: PageSize) -> Option<Self> {
        if !range.base.is_aligned_to(page_size.align()) {
            return None;
        }

        let base = PageAddr::new(range.base, page_size);
        if range.size % page_size.align() != 0 {
            return None;
        }

        let len = range.size / page_size.align();

        Some(Self { base, len })
    }
}

pub trait PageManager<S: AddrSpace> {
    /// Allocates contiguous `cnt` of `page_size`-sized pages
    ///
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_pages(&mut self, cnt: usize, page_size: PageSize) -> Option<PageRange<S>>;

    // /// Allocates contiguous `cnt` of `page_size`-sized pages which starts
    // /// at `at`. If the `cnt` pages starting at `at` is not available to
    // /// allocate, this tries to allocate some other contiguous pages.
    // fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at:
    // PageAddr<S>) -> Option<PageRange<S>>;

    /// Deallocate `page`
    ///
    /// # Safety
    /// `page` should be a page allocated by this allocator.
    unsafe fn deallocate_pages(&mut self, pages: PageRange<S>);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageSize {
    Small,
    Large,
    Huge,
}
impl PageSize {
    const MAX: Self = Self::Huge;
    const MIN: Self = Self::Small;

    pub const fn align(self) -> usize { self.usize() }

    pub const fn order(self) -> u8 { self.usize().trailing_zeros() as u8 }

    pub const fn usize(self) -> usize {
        match self {
            PageSize::Small => 4 * KiB,
            PageSize::Large => 2 * MiB,
            PageSize::Huge => 1 * GiB,
        }
    }
}
impl Into<usize> for PageSize {
    fn into(self) -> usize { self.usize() }
}
