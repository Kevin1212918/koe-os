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
use core::ffi::CStr;
use core::fmt::Write;
use core::ptr::{slice_from_raw_parts, slice_from_raw_parts_mut};

use common::{hlt, log};
use drivers::ps2;
use io::monitor::Monitor;
use multiboot2::{BootInformation, BootInformationHeader};

use crate::common::log::{error, ok};
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

    mem::init(boot_info);
    test::test_mem();
    ok!("mem initalized");

    // Reload BootInformation using virtual address.
    let mbi_ptr = mbi_ptr as usize + PhysicalRemapSpace::OFFSET;
    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader) };
    let boot_info = boot_info.expect("boot info not found");

    if let Some(rd) = find_initrd(&boot_info) {
        fs::init_initrd(rd);
        ok!("initrd mounted");
    } else {
        error!("initrd not found");
    }

    interrupt::init();
    ok!("interrupt initialized");
    drivers::init();
    ok!("drivers initialized");
    ok!("kernel initialized");

    hlt()
}

// FIXME: Initrd memory is not handled by global allocator. This is unsafe.
fn find_initrd(boot_info: &BootInformation) -> Option<Box<[u8]>> {
    let boot_mod = boot_info.module_tags().next()?;
    let data = boot_mod.start_address() as usize + PhysicalRemapSpace::OFFSET;
    let data = data as *mut u8;
    let len = boot_mod.module_size() as usize;
    let slice = slice_from_raw_parts_mut(data, len);

    // Not safe!
    Some(unsafe { Box::from_raw(slice) })
}
