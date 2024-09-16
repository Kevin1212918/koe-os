#![no_std]
#![no_main] 

mod multiboot2_header; 
mod drivers;

use core::{arch::global_asm, panic::PanicInfo};

global_asm!(include_str!("boot.S"));

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop{}
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGABuffer::new();
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    vga_buffer.write(b"Hello World!");

    loop {}
}
