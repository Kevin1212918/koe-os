use bitflags::{bitflags, Flags as _};
use derive_more::derive::{From, Into};

use super::Level;
use crate::common::{GiB, KiB, MiB};
use crate::mem::addr::Addr;
use crate::mem::UMASpace;


/// A raw paging table entry without type info.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Into, From)]
pub struct RawEntry(pub usize);
impl RawEntry {
    pub const fn default() -> Self { Self(0) }
}


/// A mutable reference to paging table entry with metadata.
#[derive(Debug)]
pub struct EntryRef<'a> {
    level: Level,
    raw: &'a mut RawEntry,
}
impl<'a> From<EntryRef<'a>> for &'a mut RawEntry {
    fn from(val: EntryRef<'a>) -> Self { val.raw }
}

impl<'a> EntryRef<'a> {
    pub fn is_present(&self) -> bool { !matches!(self.target(), EntryTarget::None) }

    pub fn is_page(&self) -> bool { matches!(self.target(), EntryTarget::Page(..)) }

    pub fn is_table(&self) -> bool { matches!(self.target(), EntryTarget::Table(..)) }

    pub fn level(&self) -> Level { self.level }

    pub fn raw(self) -> &'a mut RawEntry { self.raw }

    /// Get the referenced target for `Entry`
    pub fn target(&self) -> EntryTarget {
        use Level::*;

        if self.level != Level::CR3 && !self.flags().contains(Flags::PRESENT) {
            return EntryTarget::None;
        }

        let is_page = match self.level {
            CR3 | PML4 => false,
            PDPT | PD => self.flags().contains(Flags::BIG_PAGE),
            PT => true,
        };

        // Clear out 64:48 and 11:0; if a large/huge page, clear out bit 12
        // as well
        let mask = match (self.level, is_page) {
            (PDPT, true) | (PD, true) => 0x0000_FFFF_FFFF_E000,
            _ => 0x0000_FFFF_FFFF_F000,
        };

        let addr = Addr::new(self.raw.0 & mask);
        if is_page {
            EntryTarget::Page(self.level, addr)
        } else {
            let next_level = self
                .level
                .next_level()
                .expect("All level except PT should have next level");
            EntryTarget::Table(next_level, addr)
        }
    }

    pub fn set_addr(&mut self, addr: Addr<UMASpace>) -> bool {
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

    /// Get the flags from this entry. The flags may contain unspecified bits.
    pub fn flags(&self) -> Flags { Flags(self.raw.0 as u16) }

    /// Toggle the flags.
    pub fn toggle_flags(&mut self, mut flags: Flags) {
        flags.truncate();
        self.toggle_flags_unchecked(flags)
    }

    fn toggle_flags_unchecked(&mut self, flags: Flags) {
        debug_assert!(!flags.contains_unknown_bits());
        self.raw.0 ^= flags.0 as usize;
    }

    /// Set the flags to value.
    pub fn set_flags(&mut self, mut flags: Flags, value: bool) {
        flags.truncate();
        self.set_flags_unchecked(flags, value)
    }
    fn set_flags_unchecked(&mut self, flags: Flags, value: bool) {
        debug_assert!(!flags.contains_unknown_bits());
        if value {
            self.raw.0 |= flags.0 as usize;
        } else {
            // Note we cannot use flags.complement here due to unknown bits being unset.
            self.raw.0 &= !flags.0 as usize;
        }
    }

    /// Constructs a `EntryRef` from `Level` and `RawEntry`.
    ///
    /// This function is mainly useful for reconstructing `EntryRef` from
    /// already specified `RawEntry`. It is always safe to do so through this
    /// function.
    ///
    /// Note that while `raw` may or may not be specified, using `EntryRef` in
    /// any useful capacity forms requires the entry be specified through
    /// [`EntryRef::init`] or [`EntryRef::reinit`], and that the `Level` is
    /// correct.
    ///
    /// # Safety
    /// Before any functions except for [`EntryRef::init`] or
    /// [`EntryRef::reinit`] is called using the underlying raw entry, it is
    /// caller's responsibility to ensure the entry is properly specified.
    ///
    /// In addition, see [`EntryRef::init`] on safety of `level`.
    pub unsafe fn from_raw(raw: &'a mut RawEntry, level: Level) -> Self { Self { raw, level } }

    /// Specifies a `RawEntry` with given flags at `raw`, and return an
    /// `EntryRef` pointed to it. Returns `None` if the flags are not valid.
    ///
    /// # Safety
    /// Caller should ensure `level` is the correct `Level` of the containing
    /// table (or cr3).
    pub unsafe fn init(
        raw: &'a mut RawEntry,
        level: Level,
        addr: Addr<UMASpace>,
        flags: Flags,
    ) -> Self {
        // SAFETY: Specifying the raw entry.
        let mut new = unsafe { Self::from_raw(raw, level) };
        new.reinit(addr, flags);
        new
    }

    /// Respecifies the underlying `RawEntry` with `addr` and `flags`.
    pub fn reinit(&mut self, addr: Addr<UMASpace>, flags: Flags) {
        self.raw.0 = 0;
        self.set_flags(flags, true);
        self.set_addr(addr);
    }
}

/// Reference target of a paging table entry
pub enum EntryTarget {
    None,
    Table(Level, Addr<UMASpace>),
    Page(Level, Addr<UMASpace>),
}

/// Flags in a page entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Flags(u16);

bitflags! {
impl Flags: u16 {
    // Universal set_flags
    const PRESENT = 0b1;
    const WRITEABLE = 0b10;
    const IS_KERNEL = 0b100;
    const WRITE_THRU = 0b1000;
    const NO_CACHE = 0b1_0000;
    const ACCESSED = 0b10_0000;

    // Table/Page
    const BIG_PAGE = 0b1000_0000;

    // Page flags
    const DIRTY = 0b100_0000;
    const GLOBAL = 0b1_0000_0000;
}}

impl Flags {}
