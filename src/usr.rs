mod arch;
mod proc;

pub use proc::Task;

use crate::arch::set_kernel_entry_stack;
use crate::sched::KThread;

pub fn switch_task(next: &KThread) {
    let stack_base = next.stack_base();
    unsafe { set_kernel_entry_stack(stack_base as usize, None) };
}
