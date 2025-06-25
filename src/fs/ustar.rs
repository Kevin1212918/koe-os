use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::sync::Arc;
use core::ffi::CStr;
use core::{ptr, slice, str};

use bitflags::bitflags;

use super::{Error, FileSystem, INode, Result};

pub const BLOCK_SIZE: usize = 512;

#[derive(Clone)]
pub struct UStarFs(Arc<spin::Mutex<Box<[u8]>>>);
pub struct UStarNode {
    fs: UStarFs,
    header_off: usize,
}

impl UStarFs {
    pub fn new(buf: Box<[u8]>) -> Self { Self(Arc::new(spin::Mutex::new(buf))) }
    fn find_header_off<'a, 'z>(tape: &'a [u8], path: &'z str) -> Option<usize> {
        let mut header_off = 0;
        while header_off < tape.len() {
            let header = unsafe {
                tape.as_ptr()
                    .byte_add(header_off)
                    .cast::<Header>()
                    .as_ref()
                    .unwrap()
            };
            if &header.name() == path {
                return Some(header_off);
            }
            header_off += BLOCK_SIZE + header.size().next_multiple_of(BLOCK_SIZE);
        }
        None
    }
}
impl UStarNode {
    fn header(tape: &[u8], header_off: usize) -> &Header {
        unsafe {
            tape.as_ptr()
                .byte_add(header_off)
                .cast::<Header>()
                .as_ref_unchecked()
        }
    }

    fn file(tape: &[u8], header_off: usize) -> &[u8] {
        let header = Self::header(tape, header_off);
        let start = unsafe { (header as *const Header).byte_add(BLOCK_SIZE).cast() };
        let size = header.size();
        unsafe { slice::from_raw_parts(start, size) }
    }
}

impl FileSystem for UStarFs {
    fn root(&self) -> Rc<dyn INode> {
        Rc::new(UStarNode {
            fs: self.clone(),
            header_off: 0,
        })
    }

    fn resolve(&self, path: &str) -> Option<Rc<dyn INode>> {
        let tape = self.0.lock();
        let header_off = Self::find_header_off(&tape, path)?;
        Some(Rc::new(UStarNode {
            fs: self.clone(),
            header_off,
        }))
    }
}
impl INode for UStarNode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        let tape = self.fs.0.lock();
        let file = Self::file(&tape, self.header_off);
        let available_cnt = file.len().checked_sub(offset).ok_or(Error::Unknown)?;
        let byte_cnt = usize::min(buf.len(), available_cnt);
        buf[0..byte_cnt].copy_from_slice(&file[offset..offset + byte_cnt]);
        Ok(byte_cnt)
    }
}

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
