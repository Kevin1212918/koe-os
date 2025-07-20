use alloc::vec::Vec;
use core::alloc::Layout;

use hashbrown::HashMap;

use super::mmap::MMap;
use crate::arch::set_kernel_entry_stack;
use crate::common::log::info;
use crate::common::StackPtr;
use crate::fs::{load_elf, File};
use crate::interrupt::IntrptGuard;
use crate::mem::addr::{Addr, Page, PageSize};
use crate::mem::paging::{Attribute, MemoryMap as _};
use crate::mem::{PageAllocator, Paging, UserSpace};
use crate::sched::{KThread, Scheduler, ThreadId};
use crate::usr::arch::enter_usr;

const STACK_BASE: StackPtr = 0x0000_7FFF_C000_0000;
const STACK_PAGE_CNT: usize = 4;
const STACK_SIZE: usize = STACK_PAGE_CNT * PageSize::MIN.usize();
const STACK_ALIGN: usize = PageSize::MIN.align();
const STACK_ATTR: Attribute = Attribute::WRITEABLE
    .union(Attribute::WRITE_BACK)
    .union(Attribute::IS_USR);

static TASK_MAP: spin::Lazy<spin::Mutex<HashMap<ThreadId, Task>>> =
    spin::Lazy::new(|| spin::Mutex::new(HashMap::new()));

pub type Fd = usize;

pub struct Task {
    pub files: Vec<File>,
    pub mmap: MMap,
    pub thread_id: ThreadId,
}
impl Task {
    pub fn launch(path: &str) -> ThreadId {
        let paging = Paging::new();

        let stack_layout = Layout::from_size_align(STACK_SIZE, STACK_ALIGN).unwrap();
        let stack_ppages = PageAllocator
            .allocate_pages(stack_layout)
            .expect("User stack allocation failed.");

        let stack_vlo = Addr::<UserSpace>::new(STACK_BASE - STACK_SIZE);

        let mut mmap = MMap::empty(paging);
        unsafe {
            mmap.raw_map(
                Some(stack_vlo),
                stack_ppages,
                STACK_ATTR,
            )
        };
        let mut files = Vec::with_capacity(0);
        files.push(File::open(path).expect("Exec file not found!"));
        // FIXME: Race condition here where scheduler schedules the task before
        // it is inserted into task table.
        let thread_id = Scheduler::new().launch(task_entry, 1, true);
        let task = Self {
            files,
            mmap,
            thread_id,
        };
        TASK_MAP.lock().insert(thread_id, task);
        thread_id
    }
}

fn task_entry() {
    info!("Entering a task");
    let tid = KThread::my_tid();
    let mut task_map = TASK_MAP.lock();
    let task = task_map.get_mut(&tid).expect("Race condition strikes!");
    let instr_ptr = load_elf(task, 0).expect("Elf loading failed");
    let stack_ptr = STACK_BASE;
    drop(task_map);

    unsafe { enter_usr(stack_ptr, instr_ptr) };
}

pub fn switch_task(next_task: &KThread) {
    // FIXME: deadlock on MMU lock
    let stack_base = next_task.stack_base();
    unsafe { set_kernel_entry_stack(stack_base as usize, None) };
    let task_map = TASK_MAP.lock();
    let cpu_id = KThread::my_cpu_id();
    let task = task_map.get(&next_task.tid()).unwrap();
    task.mmap.activate(cpu_id);
}
