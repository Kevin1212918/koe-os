use alloc::vec::Vec;
use core::alloc::{Allocator, Layout};

use crate::arch::hlt;
use crate::common::log::{info, ok};
use crate::mem::SlabAllocator;
use crate::sched::{self, KThread, Scheduler, ThreadState};

pub fn test_mem() {
    // FIXME: reorganize test cases
    let mut test = Vec::new();
    for i in 0..2000 {
        test.push(i);
    }
    let mut test2: Vec<u32> = Vec::new();
    for i in 0..2000 {
        test2.push(i);
    }
    for i in test.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    drop(test);
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }

    let mut ptrs = Vec::new();
    let lay = Layout::from_size_align(8, 8).unwrap();
    for i in 0..1000 {
        ptrs.push(
            SlabAllocator
                .allocate(lay)
                .expect("alloc should be successful"),
        );
    }
    for p in ptrs {
        unsafe { SlabAllocator.deallocate(p.as_non_null_ptr(), lay) };
    }
}

pub fn test_kthread() {
    Scheduler::new().launch(task2, 1);
    Scheduler::new().launch(task1, 1);
    Scheduler::new().launch(task1, 1);
}

fn task1() {
    ok!("executing task1");
    Scheduler::new().launch(task4, 1);
    Scheduler::new().launch(task3, 1);
    Scheduler::new().launch(task3, 1);
    Scheduler::new().launch(task2, 1);
}

fn task3() {
    ok!("executing task3");
}

fn task4() {
    ok!("executing task4");
    for i in 0..50 {
        if i % 10 == 0 {}
        hlt();
    }
}

fn task2() {
    ok!("executing task2");
    for i in 0..50 {
        if i % 10 == 0 {}
        hlt();
    }
}
