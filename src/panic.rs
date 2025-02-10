use core::fmt::Write as _;
use core::panic::PanicInfo;

use crate::common::hlt;
use crate::drivers;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use drivers::vga::*;
    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.clear();
    vga_buffer.set_color(Color::Red, Color::Black, true);
    write!(
        *vga_buffer,
        "KERNEL PANIC: {}",
        info.message()
    );
    drop(vga_buffer);
    hlt()
}
