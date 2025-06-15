use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;

use super::initramfs;


trait FileSystem {
    fn root(&self) -> Rc<dyn INode>;
    fn resolve(&self, path: &str) -> Option<Rc<dyn INode>>;
}

trait INode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Option<()>;
    fn stat(&self) -> Stat;
    fn lookup(&self, name: &str) -> Option<Rc<dyn INode>>;
}

struct Stat {}


pub struct Vfs {
    mounts: Vec<Box<dyn FileSystem>>,
}
impl FileSystem for Vfs {
    fn root(&self) -> Rc<dyn INode> { self.mounts[0].root() }

    fn resolve(&self, path: &str) -> Option<Rc<dyn INode>> { self.mounts[0].resolve(path) }
}

/// Read only file backed by an inode.
pub struct File {
    pos: usize,
    inode: Rc<dyn INode>,
}

impl File {
    pub fn open(vfs: &Vfs, path: &str) -> Option<Self> {
        let inode = vfs.resolve(path)?;
        Some(Self { pos: 0, inode })
    }
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        // FIXME: underlying fs need to still exist.

        match self.inode.read(self.pos, buf) {
            None => 0,
            Some(()) => buf.len(),
        }
    }
}
