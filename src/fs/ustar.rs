use alloc::string::String;
use core::ffi::CStr;
use core::str;

use bitflags::bitflags;

pub const BLOCK_SIZE: usize = 512;
#[repr(C, packed)]
pub struct Header {
    pub name: [u8; 100],
    pub mode: [u8; 8],
    pub uid: [u8; 8],
    pub gid: [u8; 8],
    pub size: [u8; 12],
    mtime: [u8; 12],
    chksum: [u8; 8],
    typeflag: TypeFlag,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],

    _reserved: [u8; 12],
}

impl Header {
    const MAGIC: &'static [u8; 6] = b"ustar\0";
    const VERSION: &'static [u8; 2] = b"00";
    const NAME_MAX_SIZE: usize = 255;

    pub fn name(&self) -> String {
        let suffix = match CStr::from_bytes_until_nul(&self.name) {
            Ok(cs) => cs.to_str(),
            Err(_) => str::from_utf8(&self.name),
        }
        .expect("Header name should be valid utf8");

        let prefix = match CStr::from_bytes_until_nul(&self.prefix) {
            Ok(cs) => cs.to_str(),
            Err(_) => str::from_utf8(&self.prefix),
        }
        .expect("Header prefix should be valid utf8");

        let mut res = String::with_capacity(suffix.len() + prefix.len());
        res.push_str(prefix);
        res.push_str(suffix);
        res
    }

    pub fn size(&self) -> usize { octal2usize(&self.size[0..11]) }
}

fn octal2usize(octal: &[u8]) -> usize {
    octal.iter().fold(0, |sum, digit| {
        sum * 8 + (digit - b'0') as usize
    })
}

#[repr(u8)]
enum TypeFlag {
    Normal = b'0',
    Normal2 = b'\0',
    HardLink = b'1',
    SymLink = b'2',
    CharDev = b'3',
    BlockDev = b'4',
    Directory = b'5',
    Pipe = b'6',
}

bitflags! {
struct Mode: u64 {
    const SUID = 0o4000;
    const SGID = 0o2000;
    const SVTX = 0o1000;

    const UREAD = 0o0400;
    const UWRITE = 0o0200;
    const UEXEC = 0o0100;

    const GREAD = 0o0040;
    const GWRITE = 0o0020;
    const GEXEC = 0o0010;

    const OREAD = 0o0004;
    const OWRITE = 0o0002;
    const OEXEC = 0o0001;
}}
