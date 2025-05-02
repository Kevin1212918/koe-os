pub trait FileSystem {
    type INode: INode;

    fn root(&self) -> &Self::INode;
}

pub trait INode {
    type FileSystem: FileSystem;

    fn lookup<'a, 'z>(&'a self, fs: &'a Self::FileSystem, name: &'z str) -> Option<&'a Self>;
}

pub struct File {}
