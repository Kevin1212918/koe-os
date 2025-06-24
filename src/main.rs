#![no_std]
#![no_main]
#![feature(
    const_refs_to_static,
    ptr_as_ref_unchecked,
    ptr_metadata,
    impl_trait_in_assoc_type,
    sync_unsafe_cell,
    allocator_api,
    strict_overflow_ops,
    const_alloc_layout
)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use core::fmt::Write;
use core::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};

use common::{hlt, log};
use drivers::ps2;
use io::monitor::Monitor;
use multiboot2::{BootInformation, BootInformationHeader};

use crate::common::log::ok;
use crate::drivers::serial;
use crate::fs::{File, FileSystem};
use crate::mem::addr::Addr;
use crate::mem::{PhysicalRemapSpace, UMASpace};

mod boot;
mod common;
mod drivers;
mod fs;
mod interrupt;
mod io;
mod mem;
mod test;
mod usr;

#[no_mangle]
pub extern "C" fn kmain(mbi_ptr: u32) -> ! {
    serial::init();
    ok!("serial ports initialzed");

    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader) };
    let boot_info = boot_info.expect("boot info not found");
    ok!("boot info found");

    for m in boot_info.module_tags() {
        ok!(
            "found module: at {:#x} -- {:#x}",
            m.start_address(),
            m.end_address()
        )
    }


    let boot_mod = boot_info.module_tags().next().unwrap();
    ok!("initrd located");
    let initrd_addr = Addr::<UMASpace>::new(boot_mod.start_address() as usize);
    let initrd_addr = PhysicalRemapSpace::p2v(initrd_addr);
    let initrd_ptr = slice_from_raw_parts_mut(
        initrd_addr.into_ptr(),
        boot_mod.module_size() as usize,
    );

    mem::init(&boot_info);
    test::test_mem();
    ok!("mem initalized");
    interrupt::init();
    ok!("interrupt initialized");
    drivers::init();
    ok!("drivers initialized");
    ok!("kernel initialized");

    // FIXME: unsafe!
    let initrd = unsafe { Box::from_raw(initrd_ptr) };
    let initrd = fs::ustar::UStarFs::new(initrd);
    ok!("initrd loaded");

    let node = initrd.resolve("initrd/test.txt").expect("should find file");
    let mut file = File::open_with_node(node);

    let mut buf = vec![0; 100];
    let size = file.read(&mut buf).expect("read should succeed");

    let mut com1 = serial::COM1.lock();
    for b in buf {
        if b == 0 {
            break;
        }
        com1.write(b);
    }

    hlt()
}
