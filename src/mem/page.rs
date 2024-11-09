use core::{iter, ops::Range, ptr};

use bitvec::{order::Lsb0, view::BitView as _};

use crate::{common::{GiB, KiB, MiB}, mem::addr::{Addr, AddrSpace}};

/// A page consists a page aligned address and a page size
#[derive(Debug, Clone, Copy)]
pub struct Page<S: AddrSpace>{
    base: Addr<S>,
    size: PageSize,
}
impl<S: AddrSpace> Page<S> {
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
pub struct Pages<S: AddrSpace> {
    start: Addr<S>,
    end: Addr<S>,
    size: PageSize,
}
impl<S: AddrSpace> Pages<S> {
    /// Creates a contiguous range of pages between `start_page` and 
    /// `end_page`, half way inclusive.
    pub fn new(start_page: Page<S>, end_page: Page<S>) -> Self {
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
impl<S: AddrSpace> IntoIterator for Pages<S> {
    type Item = Page<S>;

    type IntoIter = iter::Map<
                        iter::StepBy<Range<usize>>, 
                        impl FnMut(usize) -> Page<S>
                    >;

    fn into_iter(self) -> Self::IntoIter {
        let start: usize = self.start.usize();
        let end: usize = self.end.usize();
        let step = self.size.usize();

        (start .. end)
            .step_by(step)
            .map(move |base| 
                Page::new(Addr::new(base), self.size)
            )
    }
}

pub trait Pager<S: AddrSpace> {
    /// Allocates contiguous `cnt` of `page_size`-sized pages
    /// 
    /// It is guarenteed that an allocated page will not be allocated again for
    /// the duration of the program.
    fn allocate_pages(&self, cnt: usize, page_size: PageSize) -> Option<Page<S>>;

    /// Allocates contiguous `cnt` of `page_size`-sized pages which starts 
    /// at `at`. If the `cnt` pages starting at `at` is not available to 
    /// allocate, this tries to allocate some other contiguous pages.
    fn allocate_pages_at(&self, cnt: usize, page_size: PageSize, at: Page<S>) -> Option<Page<S>>;

    /// Deallocate `page`
    /// 
    /// # Safety
    /// `page` should be a page allocated by this allocator.
    unsafe fn deallocate_pages(&self, page: Page<S>, cnt: usize);
}

#[repr(C)]
pub struct PageBitmap<const PAGE_SIZE: usize, S: AddrSpace> {
    /// Starting address of the memory which `PageBitmap` manages. 
    /// It is guarenteed to be `PAGE_SIZE` aligned
    base: Addr<S>,
    /// Number of pages managed by the `PageBitmap`
    len: usize,
    /// Bitmap stored as raw bytes that should be read as a `BitSlice`
    map: [u8]
}
impl<const PAGE_SIZE: usize, S: AddrSpace> PageBitmap<PAGE_SIZE, S> {
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
    pub fn managed_range(&self) -> Range<Addr<S>> {
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
        base: Addr<S>, 
        page_cnt: usize
    ) -> &'a mut PageBitmap<PAGE_SIZE, S> {
        let n_bytes_required = Self::bytes_required(page_cnt);
        let map_bytes_required = n_bytes_required - 2 * size_of::<usize>();

        let map_ptr: *mut PageBitmap<PAGE_SIZE, S> = ptr::from_raw_parts_mut(
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
    pub unsafe fn set_unchecked(&mut self, addr: Addr<S>, page_cnt: usize, is_occupied: bool) {
        let idx = addr.addr_sub(self.base) as usize / PAGE_SIZE;
        let slice = self.map.view_bits_mut::<Lsb0>();
        slice[idx..idx + page_cnt].fill(is_occupied);
    }

    /// Set the occupancy bit for all managed pages that overlaps with 
    /// `addr_start` to `addr_start + size`. Does not do anything if 
    /// `addr_start + size` is out of bound. 
    pub fn set(&mut self, addr_start: Addr<S>, size: usize, is_occupied: bool) {
        let Some(addr_end) = addr_start.checked_byte_add(size) else { return; };
        
        let section_start = addr_start.max(self.base).usize();
        let section_start = section_start - (section_start % PAGE_SIZE);

        let section_end = addr_end.min(self.base.byte_add(self.len * PAGE_SIZE)).usize();
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
                Addr::new(section_start),
                page_cnt, is_occupied
            ); 
        }
    }

    /// Returns address to the start of a section of unoccupied `page_cnt` 
    /// pages.
    /// 
    /// # Panics
    /// This panics if `page_cnt == 0`
    pub fn find_unoccupied(&self, page_cnt: usize) -> Option<Addr<S>> {
        let slice = self.map.view_bits::<Lsb0>();
        let idx = slice.windows(page_cnt)
            .enumerate()
            .find(|(_, window)| 
                window.not_any()
            )?.0;
        Some(self.base.byte_add(idx * PAGE_SIZE))
    }
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