#![no_std]
#![no_main]
#![feature(
    const_refs_to_static,
    ptr_as_ref_unchecked,
    ptr_metadata,
    impl_trait_in_assoc_type,
    sync_unsafe_cell,
    allocator_api,
    strict_overflow_ops
)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use core::fmt::Write as _;

use common::hlt;
use mem::memblock::{BootMemoryManager, BootMemoryManagerBuilder};
use multiboot2::{BootInformation, BootInformationHeader, MemoryAreaType};

mod boot;
mod common;
mod drivers;
mod mem;
mod panic;

#[no_mangle]
pub extern "C" fn kmain(mbi_ptr: u32) -> ! {
    use drivers::vga::*;

    let mut vga_buffer = VGA_BUFFER.lock();
    vga_buffer.set_color(Color::Green, Color::Black, true);
    write!(*vga_buffer, "Hello from kernel!\n").expect("VGA text mode not available");
    vga_buffer.set_color(Color::Gray, Color::Black, true);
    drop(vga_buffer);

    let boot_info = unsafe { BootInformation::load(mbi_ptr as *const BootInformationHeader) };
    let boot_info = boot_info.expect("boot info not found");
    write!(VGA_BUFFER.lock(), "boot info found\n");

    let boot_alloc = initialize_boot_memory_manager(&boot_info);
    write!(
        VGA_BUFFER.lock(),
        "boot allocator initialized\n"
    );

    mem::init(boot_info, &boot_alloc);
    write!(VGA_BUFFER.lock(), "mem initalized\n");

    drop(boot_alloc);
    hlt()
}

fn initialize_boot_memory_manager<'boot>(boot_info: &BootInformation<'boot>) -> BootMemoryManager {
    let builder = BootMemoryManagerBuilder::new()
        .expect("MemblockAllocator should not have been initialized");
    let memory_map_tag = boot_info
        .memory_map_tag()
        .expect("boot info should have memory tag");

    for area in memory_map_tag.memory_areas() {
        let base = area.start_address() as usize;
        let size = area.size() as usize;
        match area.typ().into() {
            MemoryAreaType::Available => {
                builder.add_free(base, size);
            },
            _ => {
                builder.add_reserved(base, size);
            },
        }
    }
    builder.build()
}
