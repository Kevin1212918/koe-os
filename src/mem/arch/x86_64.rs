use crate::mem::phy::BootMemoryManager;

mod paging;

pub fn init(bmm: &BootMemoryManager) { paging::init(bmm) }

pub use paging::{MemoryManager, MemoryMap, MMU};
