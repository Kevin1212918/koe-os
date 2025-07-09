use crate::mem::paging::MemoryManager;
use crate::mem::phy::BootMemoryManager;

mod paging;

pub fn init(bmm: &BootMemoryManager) -> impl MemoryManager { paging::init(bmm) }
