use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec::Vec;

use super::{FileSystem, INode, Result};

// TODO: Support multiple mounts
pub struct Vfs {
    mounts: Vec<Box<dyn FileSystem>>,
}
impl Vfs {
    pub fn mount(&mut self, fs: Box<dyn FileSystem>, path: &str) {
        assert!(path == "/");
        self.mounts.insert(0, fs);
    }
}
impl FileSystem for Vfs {
    fn root(&self) -> Rc<dyn INode> { self.mounts[0].root() }
    fn resolve(&self, path: &str) -> Option<Rc<dyn INode>> { self.mounts[0].resolve(path) }
}
