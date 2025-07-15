//! # Kernel Virtual Memory Layout
//! | Address                             | Description               | Size  |
//! |:------------------------------------|--------------------------:|:-----:|
//! |0x0000000008048000:0x????????????????|Process Data + Heap        |       |
//! |0x????????????????:0x00007FFFC0000000|Stack                      |       |



use alloc::alloc::Global;
use core::alloc::Layout;
use core::arch::asm;

use hashbrown::HashMap;

use super::arch::enter_usr;
use crate::arch::{set_kernel_entry_stack, stack_ptr};
use crate::common::log::{info, ok};
use crate::common::{InstrPtr, StackPtr};
use crate::mem::addr::{Addr, Allocator, Page, PageRange, PageSize};
use crate::mem::paging::{Attribute, MemoryMap as _};
use crate::mem::{PageAllocator, Paging, UserSpace};
use crate::sched::{KThread, Scheduler, ThreadId};
use crate::sync::spin;

pub type Pid = u32;

const PROC_START: Addr<UserSpace> = Addr::new(0x0000_0000_4000_0000);
const PROC_END: Addr<UserSpace> = Addr::new(0x0000_7FFF_8000_0000);

const STACK_BASE: StackPtr = 0x0000_7FFF_C000_0000;
const STACK_PAGE_CNT: usize = 4;
const STACK_SIZE: usize = STACK_PAGE_CNT * PageSize::MIN.usize();
const STACK_ALIGN: usize = PageSize::MIN.align();

static TASK_MAP: spin::Lazy<spin::Mutex<HashMap<ThreadId, Task>>> =
    spin::Lazy::new(|| spin::Mutex::new(HashMap::new()));

struct MMap {
    start_brk: usize,
    brk: usize,
    ustack_base: usize,
}
pub struct Task {
    paging: Paging,
    thread_id: ThreadId,
}
impl Task {
    pub fn launch() -> ThreadId {
        let paging = Paging::new();

        let stack_layout = Layout::from_size_align(STACK_SIZE, STACK_ALIGN).unwrap();
        let stack_ppages = PageAllocator
            .allocate(stack_layout)
            .expect("User stack allocation failed.");
        let stack_ppages = PageRange::try_from_range(stack_ppages, PageSize::MIN)
            .expect("PageAllocator should be page aligned.");

        let stack_vbase = Addr::<UserSpace>::new(STACK_BASE);
        let mut vpage = Page::new(stack_vbase, PageSize::MIN)
            .checked_sub(1) // Subtract 1 to get the last page.
            .unwrap();
        for ppage in stack_ppages.into_iter().rev() {
            let stack_page_attr = Attribute::WRITEABLE | Attribute::WRITE_BACK;
            unsafe {
                paging.map(
                    vpage,
                    ppage,
                    stack_page_attr,
                    &mut PageAllocator,
                );
            }
            vpage = vpage.checked_sub(1).unwrap();
        }
        let thread_id = Scheduler::new().launch(usr_entry, 1);
        let task = Self { paging, thread_id };
        TASK_MAP.lock().insert(thread_id, task);
        thread_id
    }
}

fn usr_entry() {
    info!("Entering baby user!");
    unsafe { enter_usr(STACK_BASE, baby_usr as InstrPtr) };
}

fn baby_usr() -> ! {
    loop {
        unsafe { asm!("hlt") }
    }
}
