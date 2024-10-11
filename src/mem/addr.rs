use core::{iter, ops::{BitAnd, Range, RangeBounds, RangeInclusive, Sub}, ptr};

use bitvec::{order::Lsb0, view::BitView as _};
use derive_more::derive::{From, Into};

#[allow(non_upper_case_globals)]
pub const KiB: usize = 1 << 10;
#[allow(non_upper_case_globals)]
pub const MiB: usize = 1 << 20;
#[allow(non_upper_case_globals)]
pub const GiB: usize = 1 << 30;
#[allow(non_upper_case_globals)]
pub const TiB: usize = 1 << 40;

pub trait Addr: Copy + Eq + Ord + Into<usize> + From<usize>{
    fn byte_add(self, x: usize) -> Self;
    fn checked_byte_add(self, x: usize) -> Option<Self>;
    fn byte_sub(self, x: usize) -> Self;
    fn checked_byte_sub(self, x: usize) -> Option<Self>;
    fn addr_sub(self, x: Self) -> isize;
    fn is_aligned_to(self, alignment: usize) -> bool;
}
// Workaround for const trait impl
macro_rules! const_impl_addr {
    ($struct_name: ident) => {
        impl $struct_name {
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
        impl Addr for $struct_name {
            fn byte_add(self, x: usize) -> Self {self.byte_add(x)}
            fn checked_byte_add(self, x: usize) -> Option<Self> {self.checked_byte_add(x)}
            fn byte_sub(self, x: usize) -> Self {self.byte_sub(x)}
            fn checked_byte_sub(self, x: usize) -> Option<Self> {self.checked_byte_sub(x)}
            fn addr_sub(self, x: Self) -> isize {self.addr_sub(x)}
            fn is_aligned_to(self, alignment: usize) -> bool {self.is_aligned_to(alignment)}
        }
        
    }
}

/// Address in virtual address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, From)]
pub struct VAddr(usize);    
impl VAddr {
    pub fn from_ref<T>(value: &T) -> Self {
        Self(value as *const T as usize)
    }
    pub fn into_ptr<T>(self) -> *mut T {
        self.into_usize() as *mut T
    }
}
const_impl_addr!(VAddr);
/// Address in physical address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Into, From)]
pub struct PAddr(usize);
impl PAddr {
}
const_impl_addr!(PAddr);


pub type PRange = Range<PAddr>;
pub type VRange = Range<VAddr>;
impl<T: Addr> AddrRange for Range<T> {
    type Addr = T;
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
    fn contained_pages(&self, page_size: PageSize) -> Pages<Self::Addr> {
        let start: usize = self.start.into();
        let Some(start) = start.checked_next_multiple_of(page_size.into()) else {
            return Pages::empty();
        };
        let start_page = Page::new(Self::Addr::from(start), page_size);
        
        let end: usize = self.end.into();
        let end = end - (end % page_size.into_usize());
        let end_page = Page::new(Self::Addr::from(end), page_size);

        Pages::new(start_page, end_page)
    } 
}
pub trait AddrRange: Sized {
    type Addr: Addr;
    /// Returns the set difference `self` - `rhs`. 
    /// 
    /// The set difference between two contiguous ranges result in a maximum of 
    /// two contiguous ranges, thus two ranges are returned.
    fn range_sub(&self, rhs: Self) -> [Self; 2];
    fn size(&self) -> usize;
    fn contained_pages(&self, page_size: PageSize) -> Pages<Self::Addr>;
}

/// A page consists a page aligned address and a page size
#[derive(Debug, Clone, Copy)]
pub struct Page<T: Addr>{
    base: T,
    size: PageSize,
}
impl<T: Addr> Page<T> {
    /// Creates a new page descriptor for the page at `base` of `size`
    /// 
    /// # Panics
    /// panics if `base` is not page aligned
    pub fn new(base: T, size: PageSize) -> Self {
        let alignment: usize = size.into();
        assert!(base.into() % alignment == 0);
        Self {base, size}
    }
    pub fn start(&self) -> T { self.base }
    pub fn end(&self) -> T { self.base.byte_add(self.size.into_usize()) }
    pub fn size(&self) -> PageSize { self.size } 
    pub fn range(&self) -> Range<T> { self.start() .. self.end() }
}

/// A contiguous range of pages
pub struct Pages<T: Addr> {
    start: T,
    end: T,
    size: PageSize,
}
impl<T: Addr> Pages<T> {
    /// Creates a contiguous range of pages between `start_page` and 
    /// `end_page`, half way inclusive.
    pub fn new(start_page: Page<T>, end_page: Page<T>) -> Self {
        let size = start_page.size;
        let start = start_page.base;
        let end = end_page.base;

        Self {start, end, size}
    }
    pub fn start(&self) -> T { self.start }
    pub fn page_size(&self) -> PageSize { self.size }
    pub fn len(&self) -> usize {
        (self.end.into().saturating_sub(self.start.into())) / self.size.into_usize()
    }
    pub fn empty() -> Self {
        let size = PageSize::Small;
        let start = T::from(0);
        let end = T::from(0);

        Self { start, end, size }
    }
}
impl<T: Addr> IntoIterator for Pages<T> {
    type Item = Page<T>;

    type IntoIter = iter::Map<
                        iter::StepBy<Range<usize>>, 
                        impl FnMut(usize) -> Page<T>
                    >;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.start.into();
        let end: usize = self.end.into();
        let step = self.size.into_usize();

        (start .. end)
            .step_by(step)
            .map(move |base| 
                Page::<T>::new(T::from(base), self.size)
            )
    }
}


#[repr(C)]
pub struct PageBitmap<const PAGE_SIZE: usize, A: Addr> {
    /// Starting address of the memory which `PageBitmap` manages. 
    /// It is guarenteed to be `PAGE_SIZE` aligned
    base: A,
    /// Number of pages managed by the `PageBitmap`
    len: usize,
    /// Bitmap stored as raw bytes that should be read as a `BitSlice`
    map: [u8]
}
impl<const PAGE_SIZE: usize, A: Addr> PageBitmap<PAGE_SIZE, A> {
    pub const fn align_required() -> usize {
        align_of::<usize>()
    }
    /// Calculate the byte size of `PageBitmap` for managing `page_cnt` pages
    pub const fn bytes_required(page_cnt: usize) -> usize {
        2 * size_of::<usize>() + match page_cnt {
            0 => 0,
            n => n.div_ceil(size_of::<u8>())
        }
    }
    /// Returns range of managed memory area.
    /// 
    /// The range is guarenteed to be page aligned.
    pub fn managed_range(&self) -> Range<A> {
        self.base .. self.base.byte_add(self.len * PAGE_SIZE)
    }
    /// Initialize a `PageBitmap` at `ptr` that is able to manage `page_cnt` pages
    /// starting at `base` and returning a mutable reference to the `PageBitmap`. 
    /// The map is initially fully occupied.
    /// 
    /// # Safety
    /// Let `n_bytes_required` be returned by `bytes_required(page_cnt)`. 
    /// `ptr` to `ptr + n_bytes_required` must point to an unowned, valid 
    /// region of memory.
    pub unsafe fn init<'a>(
        ptr: *mut u8, 
        base: A, 
        page_cnt: usize
    ) -> &'a mut PageBitmap<PAGE_SIZE, A> {
        let n_bytes_required = Self::bytes_required(page_cnt);
        let map_bytes_required = n_bytes_required - 2 * size_of::<usize>();

        let map_ptr: *mut PageBitmap<PAGE_SIZE, A> = ptr::from_raw_parts_mut(
            ptr, map_bytes_required
        );

        // SAFETY: PageBitmap has repr(C), thus for a PageBitMap with map size of
        // n, its layout is two usize followed by a slice of length n. 
        // n_bytes_required includes the usize fields as well, so subtracting
        // out two usize gives map_bytes_required. The memory region 
        // addr .. addr + n_bytes_required is guarenteed to be dereferencable
        // by the caller.
        let map = unsafe { map_ptr.as_mut_unchecked() };

        // The fields must be set manually because slice cannot be constructed
        map.base = base;
        map.len = page_cnt;
        map.map.fill(0xFF);
        map
    }

    /// Set the occupancy bit for the `page_cnt` pages starting with the page 
    /// pointed by `addr` 
    /// 
    /// # Safety
    /// - `addr` should be page aligned
    /// - `addr` to `addr + page_cnt * PAGE_SIZE` should be fully managed by
    /// the `PageBitmap`
    pub unsafe fn set_unchecked(&mut self, addr: A, page_cnt: usize, is_occupied: bool) {
        let idx = addr.addr_sub(self.base) as usize / PAGE_SIZE;
        let slice = self.map.view_bits_mut::<Lsb0>();
        slice[idx..idx + page_cnt].fill(is_occupied);
    }

    /// Set the occupancy bit for all managed pages that overlaps with 
    /// `addr_start` to `addr_start + size`. Does not do anything if 
    /// `addr_start + size` is out of bound. 
    pub fn set(&mut self, addr_start: A, size: usize, is_occupied: bool) {
        let Some(addr_end) = addr_start.checked_byte_add(size) else { return; };
        
        let section_start = addr_start.max(self.base).into();
        let section_start = section_start - (section_start % PAGE_SIZE);

        let section_end = addr_end.min(self.base.byte_add(self.len * PAGE_SIZE)).into();
        let section_end = section_end.next_multiple_of(PAGE_SIZE);

        // The provided section does not overlap with managed pages
        if section_start >= section_end {
            return;
        }

        let page_cnt = (section_end - section_start) / PAGE_SIZE;

        // SAFETY: section_start is page_aligned by construction. 
        // section_start >= self.base and 
        // section_end <= self.base + self.len * PAGE_SIZE, so 
        // section_start .. section_end is managed by the PageBitmap
        unsafe { 
            self.set_unchecked(
                A::from(section_start),
                page_cnt, is_occupied
            ); 
        }
    }

    /// Returns address to the start of a section of unoccupied `page_cnt` 
    /// pages.
    /// 
    /// # Panics
    /// This panics if `page_cnt == 0`
    pub fn find_unoccupied(&self, page_cnt: usize) -> Option<A> {
        let slice = self.map.view_bits::<Lsb0>();
        let idx = slice.windows(page_cnt)
            .enumerate()
            .find(|(_, window)| 
                window.not_any()
            )?.0;
        Some(self.base.byte_add(idx * PAGE_SIZE))
    }
}



pub type VPage = Page<VAddr>;
pub type PPage = Page<PAddr>;
pub type VPages = Pages<VAddr>;
pub type PPages = Pages<PAddr>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PageSize {
    Small,
    Large,
    Huge
}
impl PageSize {
    pub const fn alignment(self) -> usize {
        self.into_usize()
    }
    pub const fn into_usize(self) -> usize {
        match self {
            PageSize::Small => 4 * KiB,
            PageSize::Large => 2 * MiB,
            PageSize::Huge => 1 * GiB,
        }
    }
}
impl Into<usize> for PageSize {
    fn into(self) -> usize {
        self.into_usize()
    }
}