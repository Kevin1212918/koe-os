//! Copied from osdev

use core::fmt::Write;

use crate::common::pmio::{inb, outb, Port};

pub static COM1: spin::Mutex<Serial> = spin::Mutex::new(Serial(0));

pub fn init() {
    assert!(COM1.lock().init(0x3f8)); // COM1.
}


pub struct Serial(u16);
impl Serial {
    pub fn is_init(&self) -> bool { self.0 != 0 }
    pub fn init(&mut self, port: u16) -> bool {
        assert!(
            !self.is_init(),
            "should not reinitialize serial port"
        );


        outb(Port(port + 1), 0x00); // Disable all interrupts
        outb(Port(port + 3), 0x80); // Enable DLAB (set baud rate divisor)
        outb(Port(port + 0), 0x03); // Set divisor to 3 (lo byte) 38400 baud
        outb(Port(port + 1), 0x00); //                  (hi byte)
        outb(Port(port + 3), 0x03); // 8 bits, no parity, one stop bit
        outb(Port(port + 2), 0xC7); // Enable FIFO, clear them, with 14-byte threshold
        outb(Port(port + 4), 0x0B); // IRQs enabled, RTS/DSR set
        outb(Port(port + 4), 0x1E); // Set in loopback mode, test the serial chip
        outb(Port(port + 0), 0xAE); // Test serial chip (send byte 0xAE and check if serial returns same byte)

        // Loopback check is removed since it doesnt seem to work.

        // If serial is not faulty set it in normal operation mode
        // (not-loopback with IRQs enabled and OUT#1 and OUT#2 bits enabled)
        outb(Port(port + 4), 0x0F);

        self.0 = port;
        true
    }

    fn signal_received(&mut self) -> bool { inb(Port(self.0 + 5)) & 1 != 0 }
    pub fn read(&mut self) -> u8 {
        assert!(
            self.is_init(),
            "should not read from uninitialized serial port"
        );
        while !self.signal_received() {}
        inb(Port(self.0))
    }

    fn signal_transmitted(&mut self) -> bool { inb(Port(self.0 + 5)) & 0x20 != 0 }
    pub fn write(&mut self, byte: u8) {
        assert!(
            self.is_init(),
            "should not write to uninitialized serial port"
        );
        while !self.signal_transmitted() {}
        outb(Port(self.0), byte);
    }
}

impl Write for Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            self.write(b);
        }
        Ok(())
    }
}
