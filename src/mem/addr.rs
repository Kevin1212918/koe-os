use core::{iter, marker::PhantomData, ops::{BitAnd, Range, RangeBounds, RangeInclusive, Sub}, ptr};

use bitvec::{order::Lsb0, view::BitView as _};
use derive_more::derive::{From, Into};

use crate::common::{GiB, KiB, MiB};

use super::{phy::PhySpace, virt::VirtSpace};

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

        Self {value, _addr_space: PhantomData}
    }
    pub const fn usize(self) -> usize { self.value }

    pub fn byte_add(mut self, x: usize) -> Self {
        debug_assert!(self.value.checked_add(x).is_some_and(|v| S::RANGE.contains(&v)));

        self.value += x;
        self
    }
    pub fn checked_byte_add(mut self, x: usize) -> Option<Self> {
            self.value
                .checked_add(x)
                .filter(|x| S::RANGE.contains(x))
                .map(|x| {self.value = x; self} )
    }
    pub fn byte_sub(mut self, x: usize) -> Self {
        debug_assert!(self.value.checked_sub(x).is_some_and(|v| S::RANGE.contains(&v)));
        
        self.value -= x;
        self
    }
    pub fn checked_byte_sub(mut self, x: usize) -> Option<Self> {
            self.value
                .checked_sub(x)
                .filter(|x| S::RANGE.contains(x))
                .map(|x| {self.value = x; self} )
    }
    pub fn addr_sub(self, x: Self) -> isize {
        self.value as isize - x.value as isize
    }
    pub const fn is_aligned_to(self, alignment: usize) -> bool {
        self.value % alignment == 0
    }
}

impl<S: VirtSpace> Addr<S> {
    pub fn from_ref<T>(value: &T) -> Self {
        Addr::new(value as *const T as usize)
    }
    pub fn into_ptr<T>(self) -> *mut T {
        self.usize() as *mut T
    }
    pub fn index_range(self, range: &Range<usize>) -> usize {
        if range.is_empty() { return 0; }
        let mask = 1usize.strict_shl(range.end as u32) - 1;
        (self.value & mask) >> range.start
    }
}

impl<S: AddrSpace> AddrRange for Range<Addr<S>> {
    type Space = S; 
    fn range_sub(&self, rhs: Self) -> [Self; 2] {
        if self.is_empty() && rhs.is_empty() {
            let empty = self.start .. self.start;
            return [self.clone(), empty];
        }

        [self.start .. (self.end.min(rhs.start)),
            (self.start.max(rhs.end)) .. self.end]
    }
    fn size(&self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.end.addr_sub(self.start) as usize
        }
    }
    fn contained_pages(&self, page_size: PageSize) -> PageRange<S> {
        let start: usize = self.start.usize();
        let Some(start) = start.checked_next_multiple_of(page_size.into()) else {
            return PageRange::empty();
        };
        let start_page = PageAddr::new(Addr::new(start), page_size);
        
        let end: usize = self.end.usize();
        let end = end - (end % page_size.usize());
        let end_page = PageAddr::new(Addr::new(end), page_size);

        PageRange::new(start_page, end_page)
    }
    
}
pub trait AddrRange: Sized {
    type Space: AddrSpace;
    /// Returns the set difference `self` - `rhs`. 
    /// 
    /// The set difference between two contiguous ranges result in a maximum of 
    /// two contiguous ranges, thus two ranges are returned.
    fn range_sub(&self, rhs: Self) -> [Self; 2];
    fn size(&self) -> usize;
    fn contained_pages(&self, page_size: PageSize) -> PageRange<Self::Space>;
}

/// A page consists a page aligned address and a page size
#[derive(Debug, Clone, Copy)]
pub struct PageAddr<S: AddrSpace>{
    base: Addr<S>,
    size: PageSize,
}
impl<S: AddrSpace> PageAddr<S> {
    /// Creates a new page descriptor for the page at `base` of `size`
    /// 
    /// # Panics
    /// panics if `base` is not page aligned
    pub fn new(base: Addr<S>, size: PageSize) -> Self {
        let alignment: usize = size.into();
        assert!(base.usize() % alignment == 0);
        Self {base, size}
    }
    pub fn start(&self) -> Addr<S> { self.base }
    pub fn end(&self) -> Addr<S> { self.base.byte_add(self.size.usize()) }
    pub fn size(&self) -> PageSize { self.size } 
    pub fn range(&self) -> Range<Addr<S>> { self.start() .. self.end() }
}

/// A contiguous range of pages
pub struct PageRange<S: AddrSpace> {
    start: Addr<S>,
    end: Addr<S>,
    size: PageSize,
}
impl<S: AddrSpace> PageRange<S> {
    /// Creates a contiguous range of pages between `start_page` and 
    /// `end_page`, half way inclusive.
    pub fn new(start_page: PageAddr<S>, end_page: PageAddr<S>) -> Self {
        let size = start_page.size;
        let start = start_page.base;
        let end = end_page.base;

        Self {start, end, size}
    }
    pub fn start(&self) -> Addr<S> { self.start }
    pub fn page_size(&self) -> PageSize { self.size }
    pub fn len(&self) -> usize {
        (self.end.usize().saturating_sub(self.start.usize())) / self.size.usize()
    }
    pub fn empty() -> Self {
        let size = PageSize::Small;
        let start = Addr::new(0);
        let end = Addr::new(0);

        Self { start, end, size }
    }
}
impl<S: AddrSpace> IntoIterator for PageRange<S> {
    type Item = PageAddr<S>;

    type IntoIter = iter::Map<
                        iter::StepBy<Range<usize>>, 
                        impl FnMut(usize) -> PageAddr<S>
                    >;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.start.usize();
        let end: usize = self.end.usize();
        let step = self.size.usize();

        (start .. end)
            .step_by(step)
            .map(move |base| 
                PageAddr::new(Addr::new(base), self.size)
            )
    }
}

pub trait PageManager<S: AddrSpace> {
    /// Allocates contiguous `cnt` of `page_size`-sized pages
    /// 
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<PageAddr<S>>;

    /// Allocates contiguous `cnt` of `page_size`-sized pages which starts 
    /// at `at`. If the `cnt` pages starting at `at` is not available to 
    /// allocate, this tries to allocate some other contiguous pages.
    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: PageAddr<S>) -> Option<PageAddr<S>>;

    /// Deallocate `page`
    /// 
    /// # Safety
    /// `page` should be a page allocated by this allocator.
    unsafe fn deallocate_pages(&self, page: PageAddr<S>, cnt: usize);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageSize {
    Small,
    Large,
    Huge
}
impl PageSize {
    pub const fn alignment(self) -> usize {
        self.usize()
    }
    pub const fn usize(self) -> usize {
        match self {
            PageSize::Small => 4 * KiB,
            PageSize::Large => 2 * MiB,
            PageSize::Huge => 1 * GiB,
        }
    }
}
impl Into<usize> for PageSize {
    fn into(self) -> usize {
        self.usize()
    }
}