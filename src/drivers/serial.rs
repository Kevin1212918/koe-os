//! Copied from osdev

use crate::common::pmio::{inb, outb, Port};

const PORT: u16 = 0x3f8;
pub fn init() -> bool {
    outb(Port(PORT + 1), 0x00); // Disable all interrupts
    outb(Port(PORT + 3), 0x80); // Enable DLAB (set baud rate divisor)
    outb(Port(PORT + 0), 0x03); // Set divisor to 3 (lo byte) 38400 baud
    outb(Port(PORT + 1), 0x00); //                  (hi byte)
    outb(Port(PORT + 3), 0x03); // 8 bits, no parity, one stop bit
    outb(Port(PORT + 2), 0xC7); // Enable FIFO, clear them, with 14-byte threshold
    outb(Port(PORT + 4), 0x0B); // IRQs enabled, RTS/DSR set
    outb(Port(PORT + 4), 0x1E); // Set in loopback mode, test the serial chip
    outb(Port(PORT + 0), 0xAE); // Test serial chip (send byte 0xAE and check if serial returns same byte)

    // Check if serial is faulty (i.e: not same byte as sent)
    if inb(Port(PORT + 0)) != 0xAE {
        return true;
    }

    // If serial is not faulty set it in normal operation mode
    // (not-loopback with IRQs enabled and OUT#1 and OUT#2 bits enabled)
    outb(Port(PORT + 4), 0x0F);
    return false;
}

fn signal_received() -> bool { inb(Port(PORT + 5)) & 1 != 0 }
pub fn read() -> u8 {
    while !signal_received() {}
    inb(Port(PORT))
}

fn signal_transmitted() -> bool { inb(Port(PORT + 5)) & 0x20 != 0 }
pub fn write(byte: u8) {
    while !signal_transmitted() {}
    outb(Port(PORT), byte);
}
