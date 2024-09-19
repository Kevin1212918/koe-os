#![no_std]
#![no_main] 

mod common;
mod bootstrap; 
mod drivers;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop{}
}

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    use drivers::vga::*;

    let mut vga_buffer = unsafe { VGABuffer::init() };
        
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    vga_buffer.write(b"Hello World! ");
    vga_buffer.set_color(Color::Green, Color::Black, true);
    vga_buffer.write(b"Hello again, World!");

    loop {}
}
