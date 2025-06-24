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

use common::{hlt, log};
use drivers::ps2;
use io::monitor::Monitor;
use multiboot2::{BootInformation, BootInformationHeader};

use crate::common::log::ok;
use crate::drivers::serial;

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
    mem::init(&boot_info);
    test::test_mem();
    ok!("mem initalized");
    interrupt::init();
    ok!("interrupt initialized");
    drivers::init();
    ok!("drivers initialized");
    assert!(false);
    ok!("kernel initialized");
    hlt()
}
