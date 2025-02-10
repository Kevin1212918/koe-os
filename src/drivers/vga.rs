use core::cell::LazyCell;
use core::fmt::Write;

use crate::mem::kernel_offset_vma;

/// Address of start of VGA MMIO
const BUFFER: usize = kernel_offset_vma() + 0xb8000;

/// Length of VGA buffer in bytes
const BUFFER_LEN: usize = 0x8000; // 32 KiB

/// Height of terminal
const VIEW_HEIGHT: u8 = 25;

/// Width of terminal
const VIEW_WIDTH: u8 = 80;

pub static VGA_BUFFER: spin::Lazy<spin::Mutex<VGABuffer>> =
    spin::Lazy::new(|| spin::Mutex::new(unsafe { VGABuffer::init() }));

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
pub struct VGABuffer {
    /// Bits 3-0 represent the foreground color. Bits 6-4 represent the
    /// background color.
    color_code: u8,
    cursor_pos: u16,
    buffer: &'static mut [u16],
}
impl VGABuffer {
    /// Creates a VGABuffer.
    ///
    /// # Safety
    ///
    /// Since VGABuffer manages the MMIO of VGA graphics, there should be only
    /// one VGABuffer in existence.
    pub unsafe fn init() -> Self {
        use Color::*;

        let color_code = color_code(Black, Black, false);
        let buffer = unsafe { core::slice::from_raw_parts_mut(BUFFER as *mut u16, BUFFER_LEN / 2) };
        let filler = vga_entry(color_code, 0);
        buffer.fill(filler);

        VGABuffer {
            color_code,
            cursor_pos: 0,
            buffer,
        }
    }

    /// Clears the VGA buffer by filling it with spaces of the specified color.
    pub fn clear(&mut self) {
        let filler = vga_entry(self.color_code, 0);
        self.buffer.fill(filler);
        self.set_cursor_pos(0, 0);
    }

    pub fn set_color(&mut self, fg: Color, bg: Color, is_bright: bool) {
        self.color_code = color_code(fg, bg, is_bright);
    }

    pub fn set_cursor_pos(&mut self, x: u8, y: u8) {
        let new_pos = x as u16 * y as u16;
        assert!(new_pos < VIEW_HEIGHT as u16 * VIEW_WIDTH as u16);
        self.cursor_pos = new_pos;
    }

    pub fn get_cursor_pos(&self) -> (u8, u8) {
        let x = (self.cursor_pos % VIEW_WIDTH as u16) as u8;
        let y = (self.cursor_pos / VIEW_WIDTH as u16) as u8;
        (x, y)
    }

    pub const fn get_viewport_dim(&self) -> (u8, u8) { (VIEW_WIDTH as u8, VIEW_HEIGHT as u8) }

    pub fn write_u8(&mut self, char: u8) {
        if self.cursor_pos == VIEW_HEIGHT as u16 * VIEW_WIDTH as u16 {
            return;
        }

        match char {
            b'\n' => {
                self.cursor_pos = self.cursor_pos.next_multiple_of(VIEW_WIDTH as u16);
            },
            _ => {
                self.buffer[self.cursor_pos as usize] = vga_entry(self.color_code, char);
                self.cursor_pos += 1;
            },
        }
    }

    pub fn write(&mut self, text: &[u8]) {
        for &char in text {
            self.write_u8(char);
        }
    }
}

impl Write for VGABuffer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &c in s.as_bytes() {
            self.write_u8(c);
        }
        Ok(())
    }
}

fn vga_entry(color_code: u8, char: u8) -> u16 { ((color_code as u16) << 8) + char as u16 }
fn color_code(fg: Color, bg: Color, is_bright: bool) -> u8 {
    let mut color_code = 0;
    color_code += fg as u8;
    color_code += (is_bright as u8) << 3;
    color_code += (bg as u8) << 4;

    color_code
}
