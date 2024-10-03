use core::{ops::{BitAnd, Range, Sub}};

use derive_more::derive::{From, Into};

#[allow(non_upper_case_globals)]
const KiB: usize = 1 << 10;
#[allow(non_upper_case_globals)]
const MiB: usize = 1 << 20;
#[allow(non_upper_case_globals)]
const GiB: usize = 1 << 30;
#[allow(non_upper_case_globals)]
const TiB: usize = 1 << 40;

// Workaround for const trait impl
macro_rules! impl_addr {
    () => {
        pub const unsafe fn from_usize(value: usize) -> Self {
            Self(value)
        }
        pub const fn into_usize(self) -> usize {
            self.0
        }
        pub const fn byte_add(mut self, x: usize) -> Self {
            self.0 += x;
            self
        }
        pub const fn checked_byte_add(mut self, x: usize) -> Option<Self> {
            if let Some(res) = self.0.checked_add(x) {
                self.0 = res;
                Some(self)
            } else {
                None
            }
        }
        pub const fn byte_sub(mut self, x: usize) -> Self {
            self.0 -= x;
            self
        }
        pub const fn checked_byte_sub(mut self, x: usize) -> Option<Self> {
            if let Some(res) = self.0.checked_sub(x) {
                self.0 = res;
                Some(self)
            } else {
                None
            }
        }
        pub const fn addr_sub(self, x: Self) -> isize {
            self.0 as isize - x.0 as isize
        }
        pub const fn is_aligned_to(self, alignment: usize) -> bool {
            self.0 % alignment == 0
        }
    }
}

/// Address in virtual address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into)]
pub struct VAddr(usize);    
impl VAddr {
    impl_addr!();
    pub fn from_ref<T>(value: &T) -> Self {
        Self(value as *const T as usize)
    }
}
impl Addr for VAddr {}
/// Address in physical address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into)]
pub struct PAddr(usize);
impl PAddr {
    impl_addr!();
}
impl Addr for PAddr {}

pub trait Addr: Copy + Eq + Ord + Into<usize>{}

pub type PRange = Range<PAddr>;
impl AddrRange for PRange {
    fn range_sub(self, rhs: Self) -> [Self; 2] {
        if self.is_empty() && rhs.is_empty() {
            let empty = self.start .. self.start;
            return [self, empty];
        }

        [self.start .. PAddr::min(self.end, rhs.start),
         PAddr::max(self.start, rhs.end) .. self.end]
    }
}

pub type VRange = Range<VAddr>;
impl AddrRange for VRange {
    fn range_sub(self, rhs: Self) -> [Self; 2] {
        if self.is_empty() && rhs.is_empty() {
            let empty = self.start .. self.start;
            return [self, empty];
        }

        [self.start .. VAddr::min(self.end, rhs.start),
         VAddr::max(self.start, rhs.end) .. self.end]
    }
}

pub trait AddrRange: Sized {
    fn range_sub(self, rhs: Self) -> [Self; 2];
}

/// A page consists a page aligned address and a page size
pub struct Page<T: Addr>{
    pub base: T,
    pub size: PageSize,
}
impl<T: Addr> Page<T> {
    /// Creates a new page descriptor for the page at `base` of `size`
    /// 
    /// # Panics
    /// panics if `base` is not page aligned
    fn new(base: T, size: PageSize) -> Self {
        let alignment: usize = size.into();
        assert!(base.into() % alignment == 0);
        Self {base, size}
    }
}
pub type VPage = Page<VAddr>;
pub type PPage = Page<PAddr>;


#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageSize {
    Small,
    Large,
    Huge
}
impl Into<usize> for PageSize {
    fn into(self) -> usize {
        match self {
            PageSize::Small => 4 * KiB,
            PageSize::Large => 2 * MiB,
            PageSize::Huge => 1 * GiB,
        }
    }
}