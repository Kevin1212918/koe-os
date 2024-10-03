#![no_std]
#![no_main] 
#![feature(const_refs_to_static, ptr_as_ref_unchecked, ptr_metadata, impl_trait_in_assoc_type, sync_unsafe_cell)]
#![deny(unsafe_op_in_unsafe_fn)]

mod common;
mod bootstrap; 
mod drivers;
mod mem;

use core::{fmt::Write as _, panic::PanicInfo};

use mem::{kernel_offset_vma, kernel_start_lma, kernel_start_vma};
use multiboot2::{BootInformation, BootInformationHeader};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    use drivers::vga::*;
    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.clear();
    vga_buffer.set_color(Color::Red, Color::Black, false);
    write!(*vga_buffer, "KERNEL PANIC!!!");
    drop(vga_buffer);
    loop{}
}

#[no_mangle]
pub extern "C" fn kmain(mbi_ptr: u32) -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGA_BUFFER.lock();

    vga_buffer.set_color(Color::Green, Color::Black, true);
    write!(*vga_buffer, "Hello from kernel!\n").expect("VGA text mode not available");
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    write!(*vga_buffer, "Arg 1: {:#x}\n", mbi_ptr).expect("VGA text mode not available");

    drop(vga_buffer);

    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader).unwrap() };

    let mut vga_buffer = VGA_BUFFER.lock();

    write!(*vga_buffer, "Mem areas: {:#x?}", &boot_info.memory_map_tag().unwrap().memory_areas()[3..]);
    drop(vga_buffer);

    
    loop {}
}

