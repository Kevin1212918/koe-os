use alloc::alloc::{AllocError, Allocator};

use arrayvec::ArrayVec;

use super::{Pfn, PhysicalMemoryManager};
use crate::common::array_forest::{ArrayForest, Cursor};
use crate::mem::addr::PageSize;

const BUDDY_MAX_ORDER: u8 = 18;
const BUDDY_MAX_DEPTH: u8 = BUDDY_MAX_ORDER;
const BUDDY_MIN_BLOCK_SIZE: usize = PageSize::Small.usize();
const _: () = assert!(BUDDY_MAX_ORDER < u8::MAX);

pub struct BuddySystem {
    map: ArrayForest<Buddy>,
}
impl BuddySystem {
    /// Create a buddy system that manages `page_cnt` pages.
    ///
    /// # Undefined Behavior
    /// `range` should be aligned to [`BUDDY_MIN_BLOCK_SIZE`]
    ///
    /// TODO: check range in bound.
    ///
    /// # Panic
    /// See [`BitForest::new`] for `buf` requirements.
    pub fn new(page_cnt: usize, alloc: impl Allocator) -> Result<Self, AllocError> {
        let dummy_page_cnt = page_cnt.next_power_of_two();

        let max_order = (dummy_page_cnt.ilog2() as u8).max(BUDDY_MAX_ORDER);

        let tree_depth = max_order + 1;
        let tree_cnt = page_cnt.div_ceil(1 << max_order);
        let mut map = ArrayForest::new(
            tree_cnt,
            tree_depth as usize,
            alloc,
            Buddy::free(0),
        )?;
        for d in 0..tree_depth {
            // Initialize dummy pages as reserved.


            let order = max_order - d;
            let fill_cnt = page_cnt >> order;
            let partial_order = (page_cnt - (fill_cnt << order))
                .checked_ilog2()
                .unwrap_or(0) as u8;

            assert!(partial_order <= order);
            let fill = Buddy::free(order);
            let partial = Buddy::free(partial_order);
            let empty = Buddy::reserved();

            let dslice = map.slice_mut(d as usize);
            dslice[0..fill_cnt].fill(fill);
            dslice[fill_cnt] = partial;
            dslice[fill_cnt + 1..].fill(empty);
        }

        let buddy = BuddySystem { map };

        Ok(buddy)
    }

    /// Free a reserved page.
    ///
    /// # Undefined Behavior
    /// `pfn` should be previously reserved through this [`BuddySystem`]
    pub fn free(&mut self, pmm: &mut PhysicalMemoryManager, pfn: Pfn) {
        let page = pmm.page(pfn).expect("pfn should be valid.");
        let depth = Self::order2depth(&self, page.order);
        let idx = Self::pfn2idx(pfn, page.order);

        let mut cursor = self.map.cursor_mut(depth, idx);

        let buddy = cursor.get_mut();
        assert!(
            buddy.is_free(),
            "BuddySystem: Double Free!"
        );
        *buddy = Buddy::free(page.order);

        Self::fixup_map(&mut cursor);
    }

    /// Reserve a page.
    fn reserve(&mut self, pmm: &mut PhysicalMemoryManager, order: u8) -> Option<Pfn> {
        let idx = self.reserve_on_map(order)?;

        // Mark Page struct
        let pfn = Self::idx2pfn(pmm, idx, order);
        let page = pmm.page_mut(pfn).expect("idx2pfn should return valid pfn.");
        *page.order() = order;

        Some(pfn)
    }

    /// Reserve a page on map. Returns the index of the reserved buddy on map.
    fn reserve_on_map(&mut self, order: u8) -> Option<usize> {
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

            let cur_max_order = self.depth2order(cursor.depth());
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
            unreachable!("A properly sized block should be found under the root.");
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

        Some(idx)
    }

    fn pfn2idx(pfn: Pfn, order: u8) -> usize { pfn.0.get() >> order }

    fn idx2pfn(pmm: &PhysicalMemoryManager, idx: usize, order: u8) -> Pfn {
        pmm.pfn_from_raw(idx << order).unwrap()
    }

    const fn depth2order(&self, depth: usize) -> u8 { (self.map.max_depth() - depth) as u8 }

    const fn order2depth(&self, order: u8) -> usize { self.map.max_depth() - order as usize }

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
