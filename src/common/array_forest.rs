use core::{ops::{Deref, DerefMut}, ptr::NonNull};

use alloc::{alloc::{AllocError, Allocator}, slice, vec::Vec};
use bitvec::{order::Lsb0, slice::BitSlice, view::BitView};

// NOTE: Currently ArrayForest leaks memory when dropped.

/// A forest of binary trees. The forest is backed by a
/// leaked buffer.
pub struct ArrayForest<T: 'static> {
    buf: &'static mut [T],
    tree_depth: usize,
    tree_cnt: usize,
}

/// A cursor into [`ArrayForest`].
#[derive(Debug, Clone)]
pub struct Cursor<ForestRef, T: 'static>
    where ForestRef: Deref<Target = ArrayForest<T>>
{
    depth: usize,
    max_depth: usize,
    offset: usize,
    forest: ForestRef
}

impl<ForestRef, T: 'static> Cursor<ForestRef, T>
    where ForestRef: Deref<Target = ArrayForest<T>>
{   
    /// Move cursor to the left child. Returns true if successful, false if 
    /// cursor is at the last level.
    pub const fn left(&mut self) -> bool { self.down(0) }

    /// Move cursor to the right child. Returns true if successful, false if 
    /// cursor is at the last level.
    pub const fn right(&mut self) -> bool { self.down(1) }

    /// Move cursor down. Returns true if successful, false if cursor is at 
    /// the last level.
    /// 
    /// # Undefined Behavior
    /// `child_idx` should be in `0..B`
    const fn down(&mut self, child_idx: usize) -> bool {
        debug_assert!(child_idx < 2);
        if self.depth == self.max_depth { return false; }

        self.depth += 1;
        self.offset = self.offset * 2 + child_idx;
        true
    }
    
    /// Move cursor up. Returns true if successful, false if cursor is at the
    /// first level.
    pub const fn up(&mut self) -> bool {
        if self.depth == 0 { return false; }

        self.depth -= 1;
        self.offset /= 2; 
        true
    }
    
    /// Move cursor to the sibling of current node in a binary tree. Returns 
    /// true if  successful, false if cursor is at a root node.
    pub const fn sibling(&mut self) -> bool {
        if self.depth == 0 {
            false 
        } else {
            self.offset ^= 1;
            true
        }

    }

    /// Get immutable reference at cursor.
    pub fn get(&self) -> &T {
        let idx = self.offset - self.forest.tree_cnt;
        &self.forest.buf[idx]
    }

    /// Get mutable reference at cursor.
    pub fn get_mut(&mut self) -> &mut T 
        where ForestRef: DerefMut<Target = ArrayForest<T>> 
    {
        let idx = self.offset - self.forest.tree_cnt;
        &mut self.forest.buf[idx]
    }

    /// Get the depth of the cursor.
    pub const fn depth(&self) -> usize { self.depth }
    
    /// Get max depth of the cursor.
    pub const fn max_depth(&self) -> usize { self.max_depth }

    /// Get the idx of the cursor.
    pub fn idx(&self) -> usize { self.offset - self.forest.tree_cnt }
}


impl<T: 'static> ArrayForest<T> {
    const MAX_DEPTH: usize = 63;
    /// Create a [`ArrayForest`] backed by `alloc`.
    /// 
    /// # Panic 
    /// `buf` should point to a piece of memory that fits the layout returned
    /// by [`buf_layout`].
    pub fn new(
        tree_cnt: usize, 
        tree_depth: usize, 
        alloc: impl Allocator,
        fill: T,
    ) -> Result<Self, AllocError> 
        where T: Copy
    {
        let buf_layout = Self::buf_layout(tree_cnt, tree_depth);
        let len = buf_layout.size() / size_of::<T>();

        let buf_ptr = alloc.allocate(buf_layout)?.as_ptr().cast();
        let buf = unsafe { slice::from_raw_parts_mut(buf_ptr, len) };

        buf[0..len].fill(fill);
        Ok(ArrayForest{buf, tree_cnt, tree_depth})
    }

    /// Calculate the buffer layout required to back a [`ArrayForest`].
    pub fn buf_layout(tree_cnt: usize, tree_depth: usize) 
        -> core::alloc::Layout {
        
        // Note this is overestimates buffer size for simplicity
        let len = tree_cnt << tree_depth;
        core::alloc::Layout::array::<T>(len)
            .expect("ArrayForest layout should fit the memory")
    }

    /// Create a immutable cursor at the `idx` node, on `depth` level.
    /// 
    /// # Undefined Behavior
    /// If `depth` is greater than or equal to the max tree depth, or 
    /// `idx >= self.tree_cnt * Self::B.pow(depth)`, the behavior is 
    /// undefined.
    pub fn cursor<'a> (
        &'a self, 
        depth: usize, 
        idx: usize
    ) -> Cursor<&'a Self, T> {
        debug_assert!(depth <= self.tree_depth);

        let offset_start = self.offset_start(depth);
        let offset_end = self.offset_start(depth + 1);

        debug_assert!(idx < offset_end - offset_start);

        let offset = offset_start + idx;

        Cursor { 
            depth,
            max_depth: self.max_depth(),
            offset,
            forest: self
        }
    }
    
    /// Create a mutable cursor at the `idx` node, on `depth` level.
    /// 
    /// # Undefined Behavior
    /// If `depth` is greater than or equal to the max tree depth, or 
    /// `idx >= self.tree_cnt * Self::B.pow(depth)`, the behavior is 
    /// undefined.
    pub fn cursor_mut<'a> (
        &'a mut self, 
        depth: usize, 
        idx: usize
    ) -> Cursor<&'a mut Self, T> {
        debug_assert!(depth <= self.tree_depth);

        let offset_start = self.offset_start(depth);
        let offset_end = self.offset_start(depth + 1);

        debug_assert!(idx < offset_end - offset_start);

        let offset = offset_start + idx;

        Cursor { 
            depth,
            max_depth: self.max_depth(),
            offset,
            forest: self,
        }
    }

    /// Return a immutable slice to all nodes at the given depth.
    /// 
    /// # Undefined Behavior
    /// If `depth` is greater than or equal to the max tree depth, the behavior
    /// is undefined.
    pub fn slice(&self, depth: usize) -> &[T] {
        let start =  self.offset_start(depth) - self.tree_cnt;
        let end =  self.offset_start(depth+1) - self.tree_cnt;

        &self.buf[start .. end]
    }

    /// Return a mutable slice to all nodes at the given depth.
    /// 
    /// # Undefined Behavior
    /// If `depth` is greater than or equal to the max tree depth, the behavior
    /// is undefined.
    pub fn slice_mut(&mut self, depth: usize) -> &mut [T] {
        let start =  self.offset_start(depth) - self.tree_cnt;
        let end =  self.offset_start(depth+1) - self.tree_cnt;

        &mut self.buf[start .. end]
    }

    /// Returns the number of levels in a tree.
    pub const fn tree_depth(&self) -> usize { self.tree_depth }

    /// Returns the max index of levels of a tree.
    pub const fn max_depth(&self) -> usize { self.tree_depth - 1 }

    /// Returns the number of trees in the forest.
    pub const fn tree_cnt(&self) -> usize { self.tree_cnt }
    
    /// Calculate starting offset of `depth` level.
    /// 
    /// # Undefined Behavior
    /// depth should be in `0..Dpt`
    const fn offset_start(&self, depth: usize) -> usize {
        self.tree_cnt << depth
    }

    
}
