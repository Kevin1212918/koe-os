use core::{iter, marker::PhantomData, ops::{BitAnd, Range, RangeBounds, RangeInclusive, Sub}, ptr};

use bitvec::{order::Lsb0, view::BitView as _};
use derive_more::derive::{From, Into};

use crate::common::{GiB, KiB, MiB};

use super::{page::{Page, PageSize, Pages}, phy::PhySpace, virt::VirtSpace};

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
    fn contained_pages(&self, page_size: PageSize) -> Pages<S> {
        let start: usize = self.start.usize();
        let Some(start) = start.checked_next_multiple_of(page_size.into()) else {
            return Pages::empty();
        };
        let start_page = Page::new(Addr::new(start), page_size);
        
        let end: usize = self.end.usize();
        let end = end - (end % page_size.usize());
        let end_page = Page::new(Addr::new(end), page_size);

        Pages::new(start_page, end_page)
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
    fn contained_pages(&self, page_size: PageSize) -> Pages<Self::Space>;
}