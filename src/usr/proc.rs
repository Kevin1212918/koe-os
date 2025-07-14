//! # Kernel Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0x0000000008048000:0x????????????????|Process Data + Heap        |       |
//! |0x????????????????:0x00007FFFC0000000|Stack                      |       |



use crate::mem::addr::Addr;
use crate::mem::{Paging, UserSpace};
use crate::sched::ThreadId;

pub type Pid = u32;

const PROC_START: Addr<UserSpace> = Addr::new(0x0000_0000_4000_0000);
const PROC_END: Addr<UserSpace> = Addr::new(0x0000_7FFF_8000_0000);


struct MMap {
    start_brk: usize,
    brk: usize,
    ustack_base: usize,
}
struct Task {
    mmap: MMap,
    paging: Paging,
    thread_id: ThreadId,
}
impl Task {
    fn new() -> Task { todo!() }
}
