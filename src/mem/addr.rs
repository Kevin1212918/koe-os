use core::iter;
use core::marker::PhantomData;
use core::ops::{Add, Range, RangeBounds};

use super::virt::VirtSpace;
use crate::common::{GiB, KiB, MiB};

pub trait AddrSpace: Clone + Copy + PartialEq + Eq + PartialOrd + Ord {
    const RANGE: Range<usize>;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Addr<S: AddrSpace> {
    value: usize,
    _addr_space: PhantomData<S>,
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

    pub fn byte_add(mut self, x: usize) -> Self {
        debug_assert!(self
            .value
            .checked_add(x)
            .is_some_and(|v| S::RANGE.contains(&v)));

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

    pub fn byte_sub(mut self, x: usize) -> Self {
        debug_assert!(self
            .value
            .checked_sub(x)
            .is_some_and(|v| S::RANGE.contains(&v)));

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

    pub fn addr_sub(self, x: Self) -> isize { self.value as isize - x.value as isize }

    /// Returns an aligned address by rounding above.
    ///
    /// If the resulting address is higher than the address space, return the
    /// highest aligned address within the address space.
    ///
    /// # Panics
    /// Panics if no aligned address is within the address space, or alignment
    /// is 0.
    pub fn saturating_align_ceil(mut self, alignment: usize) -> Self {
        let start = S::RANGE.start;
        let end = S::RANGE.start;
        let res = self
            .usize()
            .checked_next_multiple_of(alignment)
            .filter(|&x| x < end)
            .or(Some(end - end % alignment))
            .filter(|&x| x >= start);

        self.value = res.expect("no aligned address within address space");
        self
    }

    /// Returns an aligned address by rounding below.
    ///
    /// If the resulting address is lower than the address space, return the
    /// lowest aligned address within the address space.
    ///
    /// # Panics
    /// Panics if no aligned address is within the address space, or alignment
    /// is 0.
    pub fn saturating_align_floor(mut self, alignment: usize) -> Self {
        let start = S::RANGE.start;
        let end = S::RANGE.start;
        let val = self.usize();
        let res = Some(val - val % alignment)
            .filter(|&x| x >= start)
            .or(start.checked_next_multiple_of(alignment))
            .filter(|&x| x < end);

        self.value = res.expect("no aligned address within address space");
        self
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

impl<S: AddrSpace> AddrRange for Range<Addr<S>> {
    type Space = S;

    fn start(&self) -> Addr<Self::Space> { self.start }

    fn end(&self) -> Addr<Self::Space> { self.end }

    fn range_sub(&self, rhs: Self) -> [Self; 2] {
        if self.is_empty() && rhs.is_empty() {
            let empty = self.start..self.start;
            return [self.clone(), empty];
        }

        [
            self.start..(self.end.min(rhs.start)),
            (self.start.max(rhs.end))..self.end,
        ]
    }

    fn size(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.end.addr_sub(self.start) as usize
        }
    }

    /// Returns the range of **fully** contained pages.
    fn contained_pages(&self, page_size: PageSize) -> PageRange<S> {
        if self.size() < page_size.usize() {
            return PageRange::empty(page_size);
        }

        let start = self.start.saturating_align_ceil(page_size.usize());
        let start = PageAddr::new(start, page_size);

        let end = self.end.saturating_align_floor(page_size.usize());
        let end = PageAddr::new(end, page_size);

        if end.start() <= start.start() {
            return PageRange::empty(page_size);
        }

        let size: Result<usize, _> = end.start().addr_sub(start.start()).try_into();

        match size {
            Ok(size) => PageRange::new(start, size / page_size.usize()),
            Err(_) => PageRange::empty(page_size),
        }
    }

    fn overlapped_pages(&self, page_size: PageSize) -> PageRange<Self::Space> {
        if (S::RANGE.end - S::RANGE.start) < page_size.usize() {
            return PageRange::empty(page_size);
        }

        let start = self.start.saturating_align_floor(page_size.usize());
        let start = PageAddr::new(start, page_size);

        let end = self.end.saturating_align_ceil(page_size.usize());
        let end = PageAddr::new(end, page_size);

        if end.start() <= start.start() {
            return PageRange::empty(page_size);
        }

        let size: Result<usize, _> = end.start().addr_sub(start.start()).try_into();

        match size {
            Ok(size) => PageRange::new(start, size / page_size.usize()),
            Err(_) => PageRange::empty(page_size),
        }
    }
}
pub trait AddrRange: Sized {
    type Space: AddrSpace;

    fn start(&self) -> Addr<Self::Space>;
    fn end(&self) -> Addr<Self::Space>;
    /// Returns the set difference `self` - `rhs`.
    ///
    /// The set difference between two contiguous ranges result in a maximum of
    /// two contiguous ranges, thus two ranges are returned.
    fn range_sub(&self, rhs: Self) -> [Self; 2];
    fn size(&self) -> usize;
    fn contained_pages(&self, page_size: PageSize) -> PageRange<Self::Space>;
    fn overlapped_pages(&self, page_size: PageSize) -> PageRange<Self::Space>;
}

/// A page consists a page aligned address and a page size
#[derive(Debug, Clone, Copy)]
pub struct PageAddr<S: AddrSpace> {
    base: Addr<S>,
    page_size: PageSize,
}
impl<S: AddrSpace> PageAddr<S> {
    /// Creates a new page descriptor for the page at `base` of `size`
    ///
    /// # Panics
    /// panics if `base` is not page aligned
    pub fn new(base: Addr<S>, page_size: PageSize) -> Self {
        let alignment: usize = page_size.into();
        assert!(base.usize() % alignment == 0);
        Self { base, page_size }
    }

    pub fn start(&self) -> Addr<S> { self.base }

    pub fn end(&self) -> Addr<S> { self.base.byte_add(self.page_size.usize()) }

    pub fn size(&self) -> PageSize { self.page_size }

    pub fn range(&self) -> Range<Addr<S>> { self.start()..self.end() }
}

/// A contiguous range of pages
#[derive(Debug, Clone, Copy)]
pub struct PageRange<S: AddrSpace> {
    start: PageAddr<S>,
    pub len: usize,
}
impl<S: AddrSpace> PageRange<S> {
    /// Creates a contiguous range of pages between `start_page` and
    /// `end_page`, half way inclusive.
    pub fn new(start_page: PageAddr<S>, len: usize) -> Self {
        let start = start_page;
        Self { start, len }
    }

    /// Creates a page range from addr range.
    ///
    /// Returns None if `range` is not an aligned page range.
    pub fn try_from_range(range: impl AddrRange<Space = S>, page_size: PageSize) -> Option<Self> {
        let start = range.start();
        let end = range.end();

        if !start.is_aligned_to(page_size.usize()) || !end.is_aligned_to(page_size.usize()) {
            return None;
        }

        let diff: Result<usize, _> = end.addr_sub(start).try_into();
        let Ok(diff) = diff else {
            return Some(Self::empty(page_size));
        };

        let start = PageAddr::new(start, page_size);
        (diff % page_size.usize() == 0)
            .then_some(diff / page_size.usize())
            .map(|len| Self { start, len })
    }

    /// Returns start of the `PageRange`
    pub fn start(&self) -> Addr<S> { self.start.start() }

    /// Returns end of the `PageRange`
    pub fn end(&self) -> Addr<S> { self.start.start().byte_add(self.size()) }

    pub fn range(&self) -> Range<Addr<S>> { self.start()..self.end() }

    /// Returns page size of `PageRange`
    pub fn page_size(&self) -> PageSize { self.start.size() }

    /// Returns number of pages in the `PageRange`
    pub fn len(&self) -> usize { self.len }

    /// Returns size in bytes of the `PageRange`
    pub fn size(&self) -> usize { self.len * self.start.size().usize() }

    pub fn empty(page_size: PageSize) -> Self {
        let start = PageAddr::new(Addr::new(0), page_size);

        Self { start, len: 0 }
    }

    pub fn is_empty(&self) -> bool { self.len == 0 }
}
impl<S: AddrSpace> IntoIterator for PageRange<S> {
    type IntoIter = iter::Map<iter::StepBy<Range<usize>>, impl FnMut(usize) -> PageAddr<S>>;
    type Item = PageAddr<S>;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.start().usize();
        let end: usize = self.end().usize();
        let step = self.page_size().usize();

        (start..end)
            .step_by(step)
            .map(move |base| PageAddr::new(Addr::new(base), self.page_size()))
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
    pub const fn alignment(self) -> usize { self.usize() }

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
