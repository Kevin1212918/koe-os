use super::entry::{EntryRef, RawEntry};
use super::Level;
use crate::common::KiB;
use crate::mem::addr::Addr;
use crate::mem::virt::VirtSpace;

pub const TABLE_SIZE: usize = 4 * KiB;
pub const TABLE_LEN: usize = TABLE_SIZE / size_of::<RawEntry>();
pub const TABLE_ALIGNMENT: usize = TABLE_SIZE;

// Workaround to ensure alginement of PAGE_TABLE_SIZE for PageTable
const _: () = assert!(TABLE_ALIGNMENT == 4 * KiB);
/// A paging table
#[repr(C, align(4096))]
pub struct RawTable(pub [RawEntry; TABLE_LEN]);
impl RawTable {
    pub const fn default() -> Self { Self([RawEntry::default(); TABLE_LEN]) }
}

pub struct TableRef<'a> {
    level: Level,
    data: &'a mut RawTable,
}

impl<'a> TableRef<'a> {
    pub fn raw(self) -> &'a mut RawTable { self.data }

    pub unsafe fn from_raw(level: Level, data: &'a mut RawTable) -> Self { Self { level, data } }

    /// For a `Table` of the given `typ`, get the `PageEntry` indexed by
    /// `addr`
    pub fn index_with_vaddr<S: VirtSpace>(self, addr: Addr<S>) -> EntryRef<'a> {
        let idx_range = self.level.page_table_idx_range();
        let idx = addr.index_range(&idx_range);
        self.index(idx)
    }
    pub fn index(self, idx: usize) -> EntryRef<'a> {
        debug_assert!(idx < self.data.0.len());
        let raw_entry = unsafe { self.data.0.get_unchecked_mut(idx) };
        unsafe { EntryRef::from_raw(raw_entry, self.level) }
    }
    pub fn entry_refs(self) -> impl IntoIterator<Item = EntryRef<'a>> {
        let level = self.level;
        self.data
            .0
            .iter_mut()
            .map(move |x| unsafe { EntryRef::from_raw(x, level) })
    }

    pub fn reborrow<'b>(&'b mut self) -> TableRef<'b>
    where
        'a: 'b,
    {
        TableRef {
            level: self.level,
            data: &mut self.data,
        }
    }
}
impl<'a> Into<&'a mut RawTable> for TableRef<'a> {
    fn into(self) -> &'a mut RawTable { self.data }
}
