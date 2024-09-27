use core::ops::{BitAnd, BitOr, Range, Sub};

mod phy;
mod virt;
mod alloc;
mod page;

const KERNEL_OFFSET_VMA: usize = 0xFFFFFFFF80000000;

extern "C" {
    static _KERNEL_START_VMA: u8;
    static _KERNEL_END_VMA: u8;
    static _KERNEL_START_LMA: u8;
}
#[inline]
pub const fn kernel_offset_vma() -> usize {
    KERNEL_OFFSET_VMA
}
#[inline]
pub fn kernel_start_vma() -> usize {
    unsafe {
        &_KERNEL_START_VMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_end_vma() -> usize {
    unsafe {
        &_KERNEL_END_VMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_start_lma() -> usize {
    unsafe {
        &_KERNEL_START_LMA as *const u8 as usize
    }
}
#[inline]
pub fn kernel_end_lma() -> usize {
    kernel_start_lma() + (kernel_end_vma() - kernel_start_vma())
}
#[inline]
pub fn kernel_size() -> usize {
    kernel_end_vma() - kernel_start_vma()
}
#[inline]
pub const fn kernel_data_max_vma() -> usize {
    0x40000000
}
#[inline]
pub fn kernel_data_vma() -> AddrRange {
    AddrRange::from(kernel_end_vma() .. kernel_end_vma() + kernel_data_max_vma())
}



#[derive(Debug, Clone, Copy)]
struct AddrRange {
    start: usize,
    end: usize
}
impl AddrRange {
    fn is_empty(&self) -> bool { self.start == self.end }
}
impl From<Range<usize>> for AddrRange {
    fn from(value: Range<usize>) -> Self {
        AddrRange {
            start: value.start,
            end: if value.end > value.start {value.end} else {value.start}
        }
    }
}
impl Into<Range<usize>> for AddrRange {
    fn into(self) -> Range<usize> {
        self.start .. self.end
    }
}
impl BitAnd for AddrRange {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        let start = usize::max(self.start, rhs.start);
        let end = usize::min(self.end, rhs.end);

        Self::from(start .. end)
    }
}
impl Sub for AddrRange {
    type Output = impl Iterator<Item = AddrRange>;

    fn sub(self, rhs: Self) -> Self::Output {
        let ranges: [AddrRange; 2] = [
            (self.start .. usize::min(rhs.start, self.end)).into(), 
            (self.end .. usize::max(rhs.end, self.start)).into()
        ];
        ranges.into_iter().filter(|x|!x.is_empty())
    }
}