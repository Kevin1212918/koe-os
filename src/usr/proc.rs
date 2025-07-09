//! # Kernel Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0x0000000000048000:0x????????????????|Process Data + Heap        |       |
//! |0x0000000000048000:0x????????????????|Process Data + Heap        |       |


use alloc::alloc::Global;
use alloc::boxed::Box;
use core::mem::offset_of;

use crate::common::ll::boxed::BoxLinkedListExt as _;
use crate::common::ll::{Link, Linked, LinkedList};
use crate::mem::addr::Addr;
use crate::mem::{GlobalAllocator, UserSpace};

pub type Pid = u32;

const PROC_START: Addr<UserSpace> = Addr::new(0x0000_0000_4000_0000);
const PROC_END: Addr<UserSpace> = Addr::new(0x0000_7FFF_8000_0000);
