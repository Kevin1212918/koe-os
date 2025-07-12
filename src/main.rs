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
    const_alloc_layout,
    maybe_uninit_uninit_array_transpose,
    slice_ptr_get
)]
#![allow(clippy::needless_range_loop, private_interfaces)]
#![deny(
    unsafe_op_in_unsafe_fn,
    clippy::missing_safety_doc,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::empty_docs
)]
#![warn(
    clippy::doc_link_with_quotes,
    clippy::doc_markdown,
    clippy::undocumented_unsafe_blocks
)]

extern crate alloc;

use alloc::boxed::Box;
use core::ptr::slice_from_raw_parts_mut;

use arch::die;
use multiboot2::{BootInformation, BootInformationHeader};
use test::test_kthread;

use crate::common::log::{error, ok};
use crate::drivers::serial;
use crate::mem::PhysicalRemapSpace;

mod arch;
mod boot;
mod common;
mod drivers;
mod fs;
mod interrupt;
mod io;
mod mem;
mod sched;
mod sync;
mod test;
mod usr;

#[no_mangle]
#[allow(clippy::missing_panics_doc)]
/// Kernel entry point.
pub extern "C" fn kentry(mbi_ptr: u32) -> ! {
    arch::init();

    serial::init();
    ok!("serial ports initialzed");

    // SAFETY: mbi_ptr is valid for read and not modified until boot_info is
    // consumed.
    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader) };
    let boot_info = boot_info.expect("boot info not found");
    ok!("boot info found");

    mem::init(boot_info);
    // test::test_mem();
    ok!("mem initalized");

    // Reload BootInformation using virtual address.
    let mbi_ptr = mbi_ptr as usize + PhysicalRemapSpace::OFFSET;
    // SAFETY: mbi_ptr is valid for read since it was remapped from physical memory
    // by mem::init.
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
    sched::init_scheduler(kmain);
    ok!("scheduler initialized");

    ok!("kernel initialized");
    sched::init_switch()
}

// FIXME: Initrd memory is not handled by global allocator. This is unsafe.
fn find_initrd(boot_info: &BootInformation) -> Option<Box<[u8]>> {
    let boot_mod = boot_info.module_tags().next()?;
    let data = boot_mod.start_address() as usize + PhysicalRemapSpace::OFFSET;
    let data = data as *mut u8;
    let len = boot_mod.module_size() as usize;
    let slice = slice_from_raw_parts_mut(data, len);

    // SAFETY: Not safe!
    Some(unsafe { Box::from_raw(slice) })
}

fn kmain() {
    ok!("Enter kmain");
    test_kthread();
}
