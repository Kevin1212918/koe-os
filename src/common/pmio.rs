use core::arch::asm;
use core::u8;

#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct Port(pub u16);
pub struct RPort(pub u16);
pub struct WPort(pub u16);

impl Into<RPort> for Port {
    fn into(self) -> RPort { RPort(self.0) }
}

impl Into<WPort> for Port {
    fn into(self) -> WPort { WPort(self.0) }
}

#[inline(always)]
pub fn outb(port: impl Into<WPort>, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port.into().0,
            in("al") value,
        )
    };
}

#[inline(always)]
pub fn outw(port: impl Into<WPort>, value: u16) {
    unsafe {
        asm!(
            "out dx, ax",
            in("dx") port.into().0,
            in("ax") value,
        )
    };
}

#[inline(always)]
pub fn outl(port: impl Into<WPort>, value: u32) {
    unsafe {
        asm!(
            "out dx, eax",
            in("dx") port.into().0,
            in("eax") value,
        )
    };
}

#[inline(always)]
pub fn inb(port: impl Into<RPort>) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in dx, al",
            in("dx") port.into().0,
            out("al") value,
        )
    };
    value
}

#[inline(always)]
pub fn inw(port: impl Into<RPort>) -> u16 {
    let value: u16;
    unsafe {
        asm!(
            "in dx, ax",
            in("dx") port.into().0,
            out("ax") value,
        )
    };
    value
}

#[inline(always)]
pub fn inl(port: impl Into<RPort>) -> u32 {
    let value: u32;
    unsafe {
        asm!(
            "in dx, eax",
            in("dx") port.into().0,
            out("eax") value,
        )
    };
    value
}
