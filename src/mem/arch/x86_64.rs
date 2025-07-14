use crate::mem::phy::BootMemoryManager;

mod paging;

pub fn init(bmm: &BootMemoryManager) -> impl crate::mem::paging::MemoryManager { paging::init(bmm) }

pub use paging::{MemoryManager, MemoryMap};
