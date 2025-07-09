//! # Kernel Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0x0000000000048000:0x????????????????|Process Data + Heap        |       |
//! |0x0000000000048000:0x????????????????|Process Data + Heap        |       |



use crate::mem::addr::Addr;
use crate::mem::UserSpace;

pub type Pid = u32;

const PROC_START: Addr<UserSpace> = Addr::new(0x0000_0000_4000_0000);
const PROC_END: Addr<UserSpace> = Addr::new(0x0000_7FFF_8000_0000);
