use alloc::rc::Rc;

use crate::fs::vfs::Vfs;

mod elf;
pub mod ustar;
mod vfs;



pub trait FileSystem {
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
    pub fn open_with_node(inode: Rc<dyn INode>) -> Self { Self { pos: 0, inode } }
    pub fn open(vfs: &Vfs, path: &str) -> Option<Self> {
        let inode = vfs.resolve(path)?;
        Some(Self { pos: 0, inode })
    }
    pub fn read(&mut self, buf: &mut [u8]) -> Option<usize> { self.inode.read(self.pos, buf).ok() }
    pub fn close(self) {}
}
