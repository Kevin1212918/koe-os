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

    if let Err(_) = write!(
        *vga_buffer,
        "KERNEL PANIC: {} at \n{:?}",
        info.message(),
        info.location(),
    ) {
        // I hope linter is happy >:(
    }
    drop(vga_buffer);
    hlt()
}
