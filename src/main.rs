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

use core::fmt::Write as _;

use common::hlt;
use drivers::ps2;
use io::monitor::Monitor;
use multiboot2::{BootInformation, BootInformationHeader};

mod boot;
mod common;
mod drivers;
mod interrupt;
mod io;
mod mem;
mod test;
mod usr;

#[no_mangle]
pub extern "C" fn kmain(mbi_ptr: u32) -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.set_color(Color::Green, Color::Black, true);
    write!(*vga_buffer, "Hello from kernel!\n").expect("VGA text mode not available");
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    drop(vga_buffer);

    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader) };
    let boot_info = boot_info.expect("boot info not found");

    log!("boot info found\n");

    mem::init(boot_info);
    test::test_mem();
    log!("mem initalized\n");

    interrupt::init();
    log!("interrupt initialized\n");

    drivers::init();
    log!("drivers initialized\n");

    log!("\nkernel initialized\n");
    hlt()
}
