use bitflags::bitflags;

#[repr(C, packed)]
struct Header {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
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
}

impl Header {
    const MAGIC: &'static [u8; 6] = b"ustar\0";
    const VERSION: &'static [u8; 2] = b"00";
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
