#![no_std]
#![no_main] 
#![feature(const_refs_to_static, ptr_as_ref_unchecked, ptr_metadata, impl_trait_in_assoc_type, sync_unsafe_cell, allocator_api)]
#![deny(unsafe_op_in_unsafe_fn)]

use core::fmt::Write as _;

mod common;
mod bootstrap; 
mod drivers;
mod mem;
mod panic;

#[no_mangle]
pub extern "C" fn kmain(mbi_ptr: u32) -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.set_color(Color::Green, Color::Black, true);
    write!(*vga_buffer, "Hello from kernel!\n").expect("VGA text mode not available");
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    drop(vga_buffer);

    let boot_info = mem::init(mbi_ptr as usize);
    write!(VGA_BUFFER.lock(), "mem initalized\n").expect("VGA text mode not available");
    
    loop {}
}

