#![no_std]
#![no_main] 
#![feature(const_refs_to_static)]

mod common;
mod bootstrap; 
mod drivers;
mod mem;

use core::panic::PanicInfo;

use mem::{kernel_offset_vma, kernel_start_lma, kernel_start_vma};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop{}
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    vga_buffer.write(b"Hello World! ");
    vga_buffer.set_color(Color::Green, Color::Black, true);
    vga_buffer.write(b"Hello again, World!");
    drop(vga_buffer);

    loop {}
}
