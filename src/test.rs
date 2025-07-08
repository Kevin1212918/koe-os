use alloc::vec::Vec;

use crate::arch::hlt;
use crate::common::log::ok;
use crate::sched::thread::KThread;
use crate::sched::{self, ThreadState};

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
    let mut test3 = Vec::new();
    test3.reserve_exact(43);
    for j in 0..2000 {
        let mut inner = Vec::new();
        inner.reserve_exact(15);
        for i in 0..10 {
            inner.push(i * j);
        }
        test3.push(inner);
    }
    for i in test2.iter().enumerate() {
        assert!(i.0 == *i.1 as usize);
    }
    for (j, list) in test3.iter().enumerate() {
        for (i, num) in list.iter().enumerate() {
            assert!(*num as usize == i * j);
        }
    }
}

pub fn test_kthread() {
    sched::schedule_kthread(
        KThread::boxed(task1, 1),
        ThreadState::Ready,
    );
    sched::schedule_kthread(
        KThread::boxed(task2, 1),
        ThreadState::Ready,
    );
    sched::schedule_kthread(
        KThread::boxed(task1, 1),
        ThreadState::Ready,
    );
}

fn task1() {
    ok!("executing task1");

    sched::schedule_kthread(
        KThread::boxed(task4, 1),
        ThreadState::Ready,
    );
    sched::schedule_kthread(
        KThread::boxed(task3, 1),
        ThreadState::Ready,
    );
    sched::schedule_kthread(
        KThread::boxed(task3, 1),
        ThreadState::Ready,
    );
}

fn task3() {
    ok!("executing task3");
}

fn task4() {
    ok!("executing task4");
    for i in 0..100 {
        if i % 10 == 0 {
            ok!("task4: {}th hlt", i);
        }
        hlt();
    }
}

fn task2() {
    ok!("executing task2");
    for i in 0..100 {
        if i % 10 == 0 {
            ok!("task2: {}th hlt", i);
        }
        hlt();
    }
}
