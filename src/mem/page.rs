//! x86-64 4 level ordinary paging

use core::arch::asm;

use bitvec::{array::BitArray, order::Lsb0, view::BitView};

use super::{phy::{FrameAllocator, PAddr}, virt::VAddr};

pub trait PageMapper {
    type PageSize: Ord;
    /// Returns an iterator which yields all supported page sizes in the order
    /// of smallest to largest
    fn supported_page_sizes() -> impl Iterator<Item = Self::PageSize>;
    fn is_supported(page_size: Self::PageSize) -> bool {
        Self::supported_page_sizes().any(|x|x == page_size)
    }

    /// Maps a virtual page of size `page_size` to `paddr`. Overwrite any 
    /// previous virtual page mapping at `vaddr`. 
    /// 
    /// # Safety
    /// - Virtual memory page of size `page_size` pointed by `vaddr` does not 
    /// contain any live reference or owned values.
    /// - Physical memory page of size `page_size` pointed by `paddr` does not 
    /// contain any live reference or owned values.
    /// 
    /// # Panics
    /// - `page_size` should be supported by the `PageMap`
    unsafe fn map(
        &mut self, 
        vaddr: VAddr, 
        paddr: PAddr, 
        page_size: Self::PageSize,
        allocator: &mut dyn FrameAllocator
    );

    /// Removes mapping at `vaddr`.
    /// 
    /// # Safety
    /// - Virtual memory page of size `page_size` pointed by `vaddr` does not 
    /// contain any live reference or owned values.
    unsafe fn unmap(&mut self, vaddr: VAddr);

    /// Try translating a virtual address into a physical address. Fails iff 
    /// the virtual address is not mapped.
    fn translate(&self, vaddr: VAddr) -> Option<PAddr>;
}

//---------------------------- x86-64 stuff below ---------------------------//


pub struct PageMap();
impl PageMapper for PageMap {
    type PageSize = usize;
    fn supported_page_sizes() -> impl Iterator<Item = Self::PageSize> {
        [4096].into_iter()
    }
    unsafe fn map(
        &mut self, 
        vaddr: VAddr, 
        paddr: PAddr, 
        page_size: Self::PageSize, 
        allocator: &mut dyn FrameAllocator
    ) {
        
    }

}

fn get_cr3() -> PageEntry {
    let out: usize;
    unsafe {
        asm!("mov {}, cr3", out(reg) out);
    }
    PageEntry(out)
}
unsafe fn set_cr3(ent: usize) {
    unsafe {
        asm!("mov cr3, {}", in(reg) ent);
    }
}

/// A paging structure entry.
#[repr(transparent)]
pub struct PageEntry(usize);
impl PageEntry {
    fn is_present(&self, structure: PageStructure) -> bool {
        self.get_flag(structure, PageFlag::Present).unwrap_or(true)
    }
    fn get_ref_info(&self, structure: PageStructure) -> Option<usize> {
        use PageStructure::*;

        if !self.is_present(structure) {
            return None;
        }

        // Clear out 64:48 and 11:0
        let mut addr = 0x0000_FFFF_FFFF_F000 & self.0;
        let is_page = *(unsafe { self.0.view_bits::<Lsb0>().get_unchecked(7) });

        // If a huge page, clear out bit 12
        match (structure, is_page) {
            (PDPT, true) |
            (PD, true) => { addr = addr & !(0x1000); }
            _ => ()
        }
        Some(addr)
    }
    /// Intepret the `PageEntry` as an entry in the `structure`, and try to
    /// find index of `flag` within the entry.
    /// 
    /// On success, returns the index of `flag`. Fails when such flag 
    /// cannot be found in the entry.
    fn get_flag_idx(&self, structure: PageStructure, flag: PageFlag) -> Option<usize> {
        use PageFlag::*;
        use PageStructure::*;
        
        // SAFETY: 7 is less than size of usize
        let is_page = *(unsafe { self.0.view_bits::<Lsb0>().get_unchecked(7) });
        match (structure, is_page, flag) {
            (CR3, _, WriteThru) => Some(3),
            (CR3, _, CacheDisable) => Some(4),
            (CR3, _, _) => None,

            (_, _, Present) => Some(0),
            (_, _, ReadWrite) => Some(1),
            (_, _, UserSuper) => Some(2),
            (_, _, WriteThru) => Some(3),
            (_, _, CacheDisable) => Some(4),
            (_, _, Accessed) => Some(5),

            (PD, _, PageSize) |
            (PDPT, _, PageSize) => Some(7),

            (PT, _, Dirty) |
            (PD, true, Dirty) |
            (PDPT, true, Dirty) => Some(6),
            
            (PT, _, PageAttrTbl) => Some(7),
            (PD, true, PageAttrTbl) |
            (PDPT, true, PageAttrTbl) => Some(12),

            (PT, _, Global) |
            (PD, true, Global) |
            (PDPT, true, Global) => Some(8),

            _ => None
        }
    }
    fn get_flag(&self, structure: PageStructure, flag: PageFlag) -> Option<bool> {
        let idx = self.get_flag_idx(structure, flag)?;
        let bitfield = self.0.view_bits::<Lsb0>();

        Some(unsafe { *(bitfield.get_unchecked(idx)) })

    }
}

#[derive(Debug, Clone, Copy)]
pub enum PageFlag {
    // Universal flags
    Present,
    ReadWrite,
    UserSuper,
    WriteThru,
    CacheDisable,
    Accessed,
    
    // Table/Page
    PageSize,

    // Page flags
    Dirty,
    PageAttrTbl,
    Global,
}

#[derive(Debug, Clone, Copy)]
pub enum PageStructure {
    /// Control Register 3
    CR3,
    /// Page Table
    PT,
    /// Page Directory
    PD,
    /// Page Directory Pointer Table
    PDPT,
    /// Page Map Level 4
    PML4,
}
