use alloc::vec::Vec;

pub fn test_mem() {
    // FIXME: reorganize test cases
    let mut test = Vec::new();
    for i in 0..1200 {
        test.push(i);
    }
    let mut test2: Vec<u32> = Vec::new();
    for i in 0..1200 {
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
    for j in 0..1200 {
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
