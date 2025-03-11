// TODO: refactor depth, order, and idx

use alloc::alloc::{AllocError, Allocator};

use arrayvec::ArrayVec;

use crate::common::array_forest::{ArrayForest, Cursor};
use crate::mem::addr::PageSize;
use crate::mem::paging::MemoryManager;

pub const BUDDY_MAX_ORDER: u8 = 18;
const BUDDY_MAX_DEPTH: u8 = BUDDY_MAX_ORDER;
pub const BUDDY_MIN_BLOCK_SIZE: usize = PageSize::Small.usize();
const _: () = assert!(BUDDY_MAX_ORDER < u8::MAX);

pub struct BuddySystem {
    map: ArrayForest<Buddy>,
    max_order: u8,
}
impl BuddySystem {
    /// Create a buddy system that manages `page_cnt` pages.
    ///
    /// # Panic
    /// See [`BitForest::new`] for `buf` requirements.
    pub fn new(page_cnt: usize, boot_alloc: impl Allocator) -> Result<Self, AllocError> {
        let dummy_page_cnt = page_cnt.next_power_of_two();

        let max_order = (dummy_page_cnt.ilog2() as u8).min(BUDDY_MAX_ORDER);

        let tree_depth = max_order + 1;
        let tree_cnt = page_cnt.div_ceil(1 << max_order);
        let map = ArrayForest::new(
            tree_cnt,
            tree_depth as usize,
            boot_alloc.by_ref(),
            Buddy::reserved(),
        )?;

        let buddy = BuddySystem { map, max_order };
        Ok(buddy)
    }

    /// Free a reserved page.
    ///
    /// # Safety
    /// `pfn` should have been reserved from this `BuddySystem`
    pub unsafe fn free(&mut self, idx: usize, order: u8) {
        let depth = Self::order_to_depth(&self, order);
        let idx = idx >> order;

        let mut cursor = self.map.cursor_mut(depth, idx);

        let buddy = cursor.get_mut();
        assert!(
            buddy.is_reserved(),
            "BuddySystem: Double Free!"
        );
        *buddy = Buddy::free(order);

        Self::fixup_map(&mut cursor);
    }

    /// Mark a block as free regardless of if it is previously reserved.
    ///
    /// This is used in initialization when transfering over from
    /// [`MemblockSystem`]
    ///
    /// # Safety
    /// Should not be called outside of initialization.
    pub unsafe fn free_forced(&mut self, idx: usize, order: u8) {
        assert!(order <= self.max_order);

        let max_depth = self.map.max_depth();
        let target_depth = max_depth - order as usize;
        // let idx = idx >> order;

        for depth in target_depth..=max_depth {
            let order = max_depth - depth;
            let block_cnt = 1 << (depth - target_depth);

            let start = idx >> order;
            let end = start + block_cnt;

            let blocks = self.map.slice_mut(depth);
            blocks[start..end].fill(Buddy::free(order as u8));
        }

        Self::fixup_map(&mut self.map.cursor_mut(target_depth, idx >> order));
    }

    /// Reserve a page on map. Returns the index of the reserved buddy on map.
    pub fn reserve(&mut self, order: u8) -> Option<usize> {
        assert!(order <= self.max_order);
        let mut cursor_opt = None;
        let mut stack: ArrayVec<_, { BUDDY_MAX_DEPTH as usize }> = ArrayVec::new();

        for (idx, root) in self.map.slice_mut(0).iter_mut().enumerate() {
            if root.is_free() && root.order() > order {
                cursor_opt = Some(self.map.cursor(0, idx));
                break;
            }
        }

        // No root contains a block with target order.
        if cursor_opt.is_none() {
            return None;
        }


        while let Some(ref mut cursor) = cursor_opt {
            if cursor.get().is_reserved() {
                cursor_opt = stack.pop();
                continue;
            }

            let cur_max_order = self.depth_to_order(cursor.depth());
            let cur_order = cursor.get().order();

            if cur_order < order {
                // Rewind
                cursor_opt = stack.pop();
            } else if cur_order == order && cur_order == cur_max_order {
                // Found block
                break;
            } else {
                // Keep searching
                let mut rewind = cursor.clone();
                let not_end = rewind.right();
                assert!(not_end);
                stack.push(rewind);
                cursor.left();
            }
        }

        // Allow reborrowing later
        drop(stack);

        let Some(cursor) = cursor_opt else {
            unreachable!("A fit block should be found under root with the appropriate order.");
        };
        // Found block
        let idx = cursor.idx();

        // Reborrow cursor as mut cursor
        let target_depth = cursor.depth();
        let target_idx = cursor.idx();
        let mut cursor = self.map.cursor_mut(target_depth, target_idx);

        // Update map
        *cursor.get_mut() = Buddy::reserved();
        Self::fixup_map(&mut cursor);

        Some(idx << order)
    }

    const fn depth_to_order(&self, depth: usize) -> u8 { (self.map.max_depth() - depth) as u8 }

    const fn order_to_depth(&self, order: u8) -> usize { self.map.max_depth() - order as usize }

    pub const fn max_order(&self) -> u8 { self.max_order }

    fn fixup_map(cursor: &mut Cursor<&mut ArrayForest<Buddy>, Buddy>) {
        while cursor.depth() != 0 {
            let me = *cursor.get();
            cursor.sibling();
            let sib = *cursor.get();

            let par = Buddy::max(me, sib);
            cursor.up();

            // No need to fix further
            if *cursor.get_mut() == par {
                return;
            }
            *cursor.get_mut() = par;
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Buddy(u8);
impl Buddy {
    /// Creates a free buddy.
    ///
    /// # Undefined Behavior
    /// `order` < `u8::MAX`
    const fn free(order: u8) -> Self {
        debug_assert!(order != u8::MAX);
        Buddy(order + 1)
    }

    const fn reserved() -> Self { Buddy(0) }

    const fn is_free(self) -> bool { !self.is_reserved() }

    const fn is_reserved(self) -> bool { self.0 == 0 }

    /// Returns a reference to order of a free buddy. Returns u8::MAX if buddy
    /// is not free.
    const fn order(&self) -> u8 { self.0 - 1 }
}
