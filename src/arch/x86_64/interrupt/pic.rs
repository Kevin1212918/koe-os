use super::isr::VECTOR_PIC;
use crate::arch::pmio::{inb, outb, Port};

const PIC1_CMD_PORT: Port = Port(0x20);
const PIC1_DATA_PORT: Port = Port(0x21);
const PIC2_CMD_PORT: Port = Port(0xA0);
const PIC2_DATA_PORT: Port = Port(0xA1);

// Initialization Control Words

// Initialize and send ICW4 later
const ICW1: u8 = 0b0001_0001;

// Map first 8 IRQs to VECTOR_PIC..VECTOR_PIC + 8
const ICW2_PIC1: u8 = VECTOR_PIC;

// Map next 8 IRQs to VECTOR_PIC + 8..VECTOR_PIC + 16
const ICW2_PIC2: u8 = VECTOR_PIC + 8;

// Set IRQ2 as slave PIC connection.
const ICW3_PIC1: u8 = 0b100;

// Set IRQ2 as master PIC connection.
const ICW3_PIC2: u8 = 2;

// Operate in 80x86 mode
const ICW4: u8 = 0b0000_0001;

pub fn init_pic() {
    outb(PIC1_CMD_PORT, ICW1);
    outb(PIC2_CMD_PORT, ICW1);

    outb(PIC1_DATA_PORT, ICW2_PIC1);
    outb(PIC2_DATA_PORT, ICW2_PIC2);

    outb(PIC1_DATA_PORT, ICW3_PIC1);
    outb(PIC2_DATA_PORT, ICW3_PIC2);

    outb(PIC1_DATA_PORT, ICW4);
    outb(PIC2_DATA_PORT, ICW4);
}

pub fn mask_all() {
    outb(PIC1_DATA_PORT, 0xff);
    outb(PIC2_DATA_PORT, 0xff);
}

pub fn unmask_all() {
    outb(PIC1_DATA_PORT, 0);
    outb(PIC2_DATA_PORT, 0);
}


pub fn ack(irq: u8) {
    const EOI: u8 = 0x20;
    match irq {
        0..8 => outb(PIC1_CMD_PORT, EOI),
        8..16 => outb(PIC2_CMD_PORT, EOI),
        // Don't do anything on invalid irq
        _ => (),
    }
}

pub fn mask(irq: u8) {
    let irq_offset: u8;
    let pic: Port;
    match irq {
        0..8 => {
            irq_offset = irq;
            pic = PIC1_DATA_PORT;
        },
        8..16 => {
            irq_offset = irq - 8;
            pic = PIC2_DATA_PORT;
        },
        // Don't do anything on invalid irq
        _ => return,
    }

    let mask = inb(pic);
    outb(pic, mask | 1 << irq_offset);
}

pub fn unmask(irq: u8) {
    let irq_offset: u8;
    let pic: Port;
    match irq {
        0..8 => {
            irq_offset = irq;
            pic = PIC1_DATA_PORT;
        },
        8..16 => {
            irq_offset = irq - 8;
            pic = PIC2_DATA_PORT;
        },
        // Don't do anything on invalid irq
        _ => return,
    }

    let mask = inb(pic);
    outb(pic, mask & !(1 << irq_offset));
}
