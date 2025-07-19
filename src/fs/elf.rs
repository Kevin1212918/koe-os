use alloc::vec::Vec;
use core::alloc::Layout;
use core::mem::MaybeUninit;
use core::slice;

use goblin::elf::program_header::PT_LOAD;
use goblin::elf::Elf;

use super::File;
use crate::common::InstrPtr;
use crate::mem::addr::{Addr, AddrSpace, Allocator};
use crate::mem::paging::Attribute;
use crate::mem::{PageAllocator, Paging, UserSpace};
use crate::usr::{Fd, Task};

/// Load elf
pub fn load_elf(task: &mut Task, fd: Fd) -> Option<InstrPtr> {
    let file = &mut task.files[fd];
    let size = file.inode().stat().size;
    // TODO: Parse elf incrementally
    let mut fbuf = alloc::vec![0; size];

    file.read(&mut fbuf).unwrap();

    let Ok(elf) = Elf::parse(&fbuf) else {
        return None;
    };

    // FIXME: Proper clean up, use the correct attribute
    for ph in elf.program_headers {
        if ph.p_type != PT_LOAD {
            continue;
        }
        let Ok(layout) = Layout::from_size_align(ph.p_memsz as usize, ph.p_align as usize) else {
            return None;
        };
        if !UserSpace::RANGE.contains(&(ph.p_vaddr as usize)) {
            return None;
        }

        let Some(ppages) = PageAllocator.allocate_pages(layout) else {
            return None;
        };

        let mapped = unsafe {
            task.mmap
                .raw_map(
                    Some(Addr::new(ph.p_vaddr as usize)),
                    ppages,
                    Attribute::IS_USR | Attribute::WRITEABLE | Attribute::WRITE_BACK,
                )
                .expect("MMap should succeed")
        };
        if mapped.base.addr().usize() != ph.p_vaddr as usize {
            panic!("MMap did not find memory location for elf");
        }

        let mapped = unsafe {
            slice::from_raw_parts_mut(
                mapped.base.addr().as_ptr::<MaybeUninit<u8>>(),
                ph.p_filesz as usize,
            )
        };


        MaybeUninit::copy_from_slice(
            mapped,
            &fbuf[ph.p_offset as usize..(ph.p_offset + ph.p_filesz) as usize],
        );
    }
    if elf.entry == 0 {
        None
    } else {
        Some(elf.entry as InstrPtr)
    }
}
