const BUFFER: *mut u16 = 0xb8000 as *mut u16;

/// Length of VGA buffer in bytes
const BUFFER_BYTES_LEN: usize = 0x8000; // 32 KiB

/// Height of terminal
const VIEW_HEIGHT: usize = 25;
/// Width of terminal
const VIEW_WIDTH: usize = 80;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Purple = 5,
    Brown = 6,
    Gray = 7,
}

#[repr(C)]
pub struct VGABuffer{
    /// Bits 3-0 represent the foreground color. Bits 6-4 represent the background color.
    color_code: u8,
    buffer: &'static mut[u16],
}
impl VGABuffer {
    pub fn new() -> Self {
        use Color::*;

        let color_code = color_code(Black, Black, false);
        let buffer = unsafe { core::slice::from_raw_parts_mut(BUFFER , BUFFER_BYTES_LEN/2) };
        
        let filler = vga_entry(color_code, 0);
        buffer.fill(filler);

        VGABuffer { color_code, buffer }
    }
    /// Clears the VGA buffer by filling it with spaces of the specified color.
    pub fn clear(&mut self) {
        let filler = vga_entry(self.color_code, 0);
        self.buffer.fill(filler);
    }
    pub fn write(&mut self, text: &[u8]) {
        assert!(text.len() <= BUFFER_BYTES_LEN/2);

        self.clear();
        for i in 0..text.len() {
            self.buffer[i] = vga_entry(self.color_code, text[i]);
        }
    }
    pub fn set_color(&mut self, fg: Color, bg: Color, is_bright: bool) {
        self.color_code = color_code(fg, bg, is_bright);
    }
}

fn vga_entry(color_code: u8, char: u8) -> u16 {
    ((color_code as u16) << 8) + char as u16
}
fn color_code(fg: Color, bg: Color, is_bright: bool) -> u8 {
    let mut color_code = 0;
    color_code += fg as u8;
    color_code += (is_bright as u8) << 3;
    color_code += (bg as u8) << 4;

    color_code
}