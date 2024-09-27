use derive_more::derive::{From, Into, Sub};

/// Address in virtual address space
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Into, From, PartialEq, Eq, PartialOrd, Ord, Hash, Sub)]
pub struct VAddr(usize);    
impl VAddr {
    pub fn add_offset(mut self, x: usize) -> Self {
        self.0 += x;
        self
    }
    pub fn sub_offset(mut self, x: usize) -> Self {
        self.0 -= x;
        self
    }
}

