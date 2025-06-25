use alloc::boxed::Box;
use alloc::rc::Rc;

use crate::fs::vfs::Vfs;

mod elf;
mod ustar;
mod vfs;

pub static FS: spin::Mutex<Vfs> = spin::Mutex::new(Vfs::new());

pub fn init_initrd(rd: Box<[u8]>) {
    let fs = ustar::UStarFs::new(rd);
    FS.lock().mount(Box::new(fs), "/");
}

pub trait FileSystem: Send + Sync {
    fn root(&self) -> Rc<dyn INode>;
    fn resolve(&self, path: &str) -> Option<Rc<dyn INode>>;
}

pub trait INode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize>;
    // fn stat(&self) -> Stat;
    // fn lookup(&self, name: &str) -> Option<Rc<dyn INode>>;
}

type Result<T> = core::result::Result<T, Error>;
pub enum Error {
    Unimplemented,
    Unknown,
}

struct Stat {}

/// A per-process file handle backed by a vfs inode.
pub struct File {
    pos: usize,
    inode: Rc<dyn INode>,
}
impl File {
    pub fn open(path: &str) -> Option<Self> {
        let inode = FS.lock().resolve(path)?;
        Some(Self { pos: 0, inode })
    }
    pub fn read(&mut self, buf: &mut [u8]) -> Option<usize> { self.inode.read(self.pos, buf).ok() }
    pub fn close(self) {}
}
