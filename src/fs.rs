use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::sync::Arc;
use core::borrow::Borrow;

use crate::fs::vfs::Vfs;

mod elf;
mod ustar;
mod vfs;

pub use elf::load_elf;

pub static FS: spin::Mutex<Vfs> = spin::Mutex::new(Vfs::new());

pub fn init_initrd(rd: Box<[u8]>) {
    let fs = ustar::UStarFs::new(rd);
    FS.lock().mount(Box::new(fs), "/");
}

pub trait FileSystem: Send + Sync {
    fn root(&self) -> Rc<dyn INode>;
    fn resolve(&self, path: &str) -> Option<Arc<dyn INode>>;
}

pub trait INode: Send + Sync {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize>;
    fn stat(&self) -> Stat;
    // fn lookup(&self, name: &str) -> Option<Rc<dyn INode>>;
}

type Result<T> = core::result::Result<T, Error>;
pub enum Error {
    Unimplemented,
    Unknown,
}

pub struct Stat {
    pub size: usize,
}

/// A file handle backed by a vfs inode.
pub struct File {
    pos: usize,
    inode: Arc<dyn INode>,
}
impl File {
    pub fn inode(&self) -> &dyn INode { self.inode.borrow() }

    pub fn open(path: &str) -> Option<Self> {
        let inode = FS.lock().resolve(path)?;
        Some(Self { pos: 0, inode })
    }
    pub fn read(&mut self, buf: &mut [u8]) -> Option<usize> { self.inode.read(self.pos, buf).ok() }
    pub fn close(self) {}
}
