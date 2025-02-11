use bitvec::order::Lsb0;
use bitvec::view::BitView;
use derive_more::derive::{From, Into};

use super::Level;
use crate::common::{GiB, KiB, MiB};
use crate::mem::addr::Addr;
use crate::mem::LinearSpace;


/// A raw paging table entry without type info.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Into, From)]
pub struct RawEntry(pub usize);
impl RawEntry {
    pub const fn default() -> Self { Self(0) }
}


/// A paging table entry.
#[derive(Debug)]
pub struct EntryRef<'a> {
    level: Level,
    raw: &'a mut RawEntry,
}
impl<'a> Into<&'a mut RawEntry> for EntryRef<'a> {
    fn into(self) -> &'a mut RawEntry { self.raw }
}

impl<'a> EntryRef<'a> {
    pub fn is_present(&self) -> bool { !matches!(self.target(), EntryTarget::None) }

    pub fn is_page(&self) -> bool { matches!(self.target(), EntryTarget::Page(..)) }

    pub fn is_table(&self) -> bool { matches!(self.target(), EntryTarget::Table(..)) }

    pub fn level(&self) -> Level { self.level }

    pub fn into_raw(self) -> &'a mut RawEntry { self.raw }

    /// Get the referenced target for `Entry`
    pub fn target(&self) -> EntryTarget {
        use Level::*;

        // CR3 should be the only entry without present flag, and it always
        // has a target
        if !self.flag(Flag::Present).unwrap_or(true) {
            return EntryTarget::None;
        }

        let is_page = self.flag(Flag::PageSize);

        // Clear out 64:48 and 11:0; if a large/huge page, clear out bit 12
        // as well
        let mask = match (self.level, is_page) {
            (PDPT, Some(true)) | (PD, Some(true)) => 0x0000_FFFF_FFFF_E000,
            _ => 0x0000_FFFF_FFFF_F000,
        };

        let addr = Addr::new(self.raw.0 & mask);

        match (self.level, is_page) {
            (PT, None) | (PDPT, Some(true)) | (PD, Some(true)) =>
                EntryTarget::Page(self.level, addr),

            (CR3, None) | (PML4, None) | (PDPT, Some(false)) | (PD, Some(false)) => {
                let next_level = self
                    .level
                    .next_level()
                    .expect("All level except PT should have next level");
                EntryTarget::Table(next_level, addr)
            },

            _ => unreachable!(),
        }
    }

    pub unsafe fn set_addr(&mut self, addr: Addr<LinearSpace>) -> bool {
        use EntryTarget::*;
        use Level::*;

        let target = self.target();
        let align = match (self.level, target) {
            (_, None) => return false,
            (_, Table(..)) => super::table::TABLE_ALIGNMENT,
            (PDPT, Page(..)) => 1 * GiB,
            (PD, Page(..)) => 2 * MiB,
            (PT, Page(..)) => 4 * KiB,
            _ => unreachable!(),
        };

        if !addr.is_aligned_to(align) {
            return false;
        }
        let mask = !((1 << 48) - align);

        self.raw.0 &= mask;
        self.raw.0 |= addr.usize();

        true
    }

    fn get_flag_idx(&self, flag: Flag) -> Option<usize> {
        use Flag::*;

        let data_bits = self.raw.0.view_bits::<Lsb0>();

        let present_bit = Present
            .idx(self.level, false, false)
            .map_or(false, |idx| data_bits[idx]);

        let page_size_bit = PageSize
            .idx(self.level, present_bit, false)
            .map_or(false, |idx| data_bits[idx]);

        flag.idx(self.level, present_bit, page_size_bit)
    }

    pub fn flag(&self, flag: Flag) -> Option<bool> {
        let idx = self.get_flag_idx(flag)?;
        let data_bits = self.raw.0.view_bits::<Lsb0>();

        Some(unsafe { *(data_bits.get_unchecked(idx)) })
    }

    /// Set `flags` to `value`
    ///
    /// Should not set `Present` or `PageSize` flags
    pub fn set_flags<const N: usize>(&mut self, flags: [Flag; N], value: bool) -> bool {
        let present_bit = flags
            .iter()
            .find(|&&x| matches!(x, Flag::Present))
            .is_some();
        let page_size_bit = flags
            .iter()
            .find(|&&x| matches!(x, Flag::PageSize))
            .is_some();

        if present_bit || page_size_bit {
            return false;
        }

        let prev_data = *self.raw;

        for flag in flags {
            let Some(idx) = self.get_flag_idx(flag) else {
                *self.raw = prev_data;
                return false;
            };

            let data_bits = self.raw.0.view_bits_mut::<Lsb0>();
            // SAFETY: value returned from Flag::idx should be a valid index
            unsafe { data_bits.set_unchecked(idx, value) }
        }

        true
    }

    /// Constructs a `EntryRef` from `Level` and `RawEntry`
    ///
    /// # Safety
    /// `raw` should be at level `level`.
    pub unsafe fn from_raw(raw: &'a mut RawEntry, level: Level) -> Self { Self { raw, level } }

    /// Initialize a new `RawEntry` with given flags at `raw`, and return an
    /// `EntryRef` pointed to it. Returns `None` if the flags are not valid.
    ///
    /// # Safety
    /// `addr` should point to a page table/page as specified by a `Entry`
    /// of `typ` and `flags`
    pub unsafe fn init<const N: usize>(
        raw: &'a mut RawEntry,
        level: Level,
        addr: Addr<LinearSpace>,
        flags: [Flag; N],
    ) -> Option<Self> {
        let mut new = unsafe { Self::from_raw(raw, level) };
        unsafe { new.reinit(addr, flags) }.map(|_| new)
    }

    /// Initialize a new `RawEntry` with given flags at `addr`, and return an
    /// `EntryRef` pointed to it. Returns `None` if the flags are not valid.
    ///
    /// # Safety
    /// `addr` should point to a page table/page as specified by a `Entry`
    /// of `typ` and `flags`
    pub unsafe fn reinit<const N: usize>(
        &mut self,
        addr: Addr<LinearSpace>,
        flags: [Flag; N],
    ) -> Option<()> {
        let present_bit = flags
            .iter()
            .find(|&&x| matches!(x, Flag::Present))
            .is_some();
        let page_size_bit = flags
            .iter()
            .find(|&&x| matches!(x, Flag::PageSize))
            .is_some();

        let mut data: usize = 0;
        let data_bits = data.view_bits_mut::<Lsb0>();

        for flag in flags {
            let idx = flag.idx(self.level, present_bit, page_size_bit)?;

            // SAFETY: value returned from Flag::idx should be a valid index
            unsafe { data_bits.set_unchecked(idx, true) };
        }

        self.raw.0 = data;
        unsafe { self.set_addr(addr) }.then_some(())
    }
}

/// Reference target of a paging table entry
pub enum EntryTarget {
    None,
    Table(Level, Addr<LinearSpace>),
    Page(Level, Addr<LinearSpace>),
}

/// A flag in a page entry. Currently supports `Present`, `ReadWrite`,
/// `UserSuper`, `PageSize`, `Global`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, From)]
pub enum Flag {
    // Universal set_flags
    Present,
    ReadWrite,
    UserSuper,
    // WriteThru,
    // CacheDisable,
    // Accessed,

    // Table/Page
    PageSize,

    // Page flags
    // Dirty,
    // PageAttrTbl,
    Global,
}
impl Flag {
    fn idx(self, level: Level, present_bit: bool, page_size_bit: bool) -> Option<usize> {
        use Flag::*;
        use Level::*;

        return match level {
            CR3 => cr3_idx(self, present_bit, page_size_bit),
            PML4 => pml4_idx(self, present_bit, page_size_bit),
            PDPT => pdpt_idx(self, present_bit, page_size_bit),
            PD => pd_idx(self, present_bit, page_size_bit),
            PT => pt_idx(self, present_bit, page_size_bit),
        };

        fn cr3_idx(_: Flag, _: bool, _: bool) -> Option<usize> { None }

        fn pml4_idx(flag: Flag, present_bit: bool, _: bool) -> Option<usize> {
            match (present_bit, flag) {
                (false, Present) => Some(0),
                (false, _) => None,

                (true, Present) => Some(0),
                (true, ReadWrite) => Some(1),
                (true, UserSuper) => Some(2),
                (true, _) => None,
            }
        }

        fn pdpt_idx(flag: Flag, present_bit: bool, page_size_bit: bool) -> Option<usize> {
            match (present_bit, page_size_bit, flag) {
                (false, _, Present) => Some(0),
                (false, _, _) => None,

                (true, true, Present) => Some(0),
                (true, true, ReadWrite) => Some(1),
                (true, true, UserSuper) => Some(2),
                (true, true, PageSize) => Some(7),
                (true, true, Global) => Some(8),

                (true, false, Present) => Some(0),
                (true, false, ReadWrite) => Some(1),
                (true, false, UserSuper) => Some(2),
                (true, false, PageSize) => Some(7),
                (true, false, _) => None,
            }
        }

        fn pd_idx(flag: Flag, present_bit: bool, page_size_bit: bool) -> Option<usize> {
            match (present_bit, page_size_bit, flag) {
                (false, _, Present) => Some(0),
                (false, _, _) => None,

                (true, true, Present) => Some(0),
                (true, true, ReadWrite) => Some(1),
                (true, true, UserSuper) => Some(2),
                (true, true, PageSize) => Some(7),
                (true, true, Global) => Some(8),

                (true, false, Present) => Some(0),
                (true, false, ReadWrite) => Some(1),
                (true, false, UserSuper) => Some(2),
                (true, false, PageSize) => Some(7),
                (true, false, _) => None,
            }
        }

        fn pt_idx(flag: Flag, present_bit: bool, _: bool) -> Option<usize> {
            match (present_bit, flag) {
                (false, Present) => Some(0),
                (false, _) => None,

                (true, Present) => Some(0),
                (true, ReadWrite) => Some(1),
                (true, UserSuper) => Some(2),
                (true, Global) => Some(8),
                (true, _) => None,
            }
        }
    }
}
