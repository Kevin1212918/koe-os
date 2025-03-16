use core::arch::asm;
use core::u8;

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Port(pub u16);

#[inline(always)]
pub fn outb(port: Port, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port.0,
            in("al") value,
        )
    };
}

#[inline(always)]
pub fn outw(port: Port, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port.0,
            in("ax") value,
        )
    };
}

#[inline(always)]
pub fn outl(port: Port, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port.0,
            in("eax") value,
        )
    };
}

#[inline(always)]
pub fn inb(port: Port) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in dx, al",
            in("dx") port.0,
            out("al") value,
        )
    };
    value
}

#[inline(always)]
pub fn inw(port: Port) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in dx, ax",
            in("dx") port.0,
            out("ax") value,
        )
    };
    value
}

#[inline(always)]
pub fn inl(port: Port) -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "in dx, eax",
            in("dx") port.0,
            out("eax") value,
        )
    };
    value
}
