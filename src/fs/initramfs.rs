use alloc::alloc::dealloc;
use alloc::boxed::Box;
use core::alloc::{Allocator, Layout};
use core::cell::SyncUnsafeCell;
use core::ptr::{self, NonNull};
use core::str;

use super::*;
use crate::fs::vfs::File;

pub static FS: spin::Mutex<Option<InitRamFs>> = spin::Mutex::new(None);

pub struct InitRamFs {
    pub tape: &'static [u8],
}
impl InitRamFs {
    /// Mount the buffer at `tape` as the init ramdisk. [`FS`] will be populated
    /// on a successful call.
    ///
    /// # Safety
    /// - `tape` is allocated on the global allocator.
    pub unsafe fn mount(tape: NonNull<[u8]>) {
        // TODO: validate tape format

        *FS.lock() = Some(Self {
            tape: unsafe { tape.as_ref() },
        });
    }
}

impl Drop for InitRamFs {
    fn drop(&mut self) {
        let tape = self.tape.as_ptr().cast_mut();

        // SAFETY: Guarenteed by `Self::mount` that the backing storage is allocated on
        // the global allocator.

        // FIXME: not safe when dropped while referencing file still lives.
        unsafe { dealloc(tape, Layout::for_value(self.tape)) };
    }
}

impl InitRamFs {
    pub fn lookup<'a, 'z>(&'a self, path: &'z str) -> Option<&'a ustar::Header> {
        let mut header_off = 0;
        while header_off < self.tape.len() {
            let header = unsafe {
                self.tape
                    .as_ptr()
                    .byte_add(header_off)
                    .cast::<ustar::Header>()
                    .as_ref()
                    .unwrap()
            };
            if &header.name() == path {
                return Some(header);
            }
            header_off += ustar::BLOCK_SIZE + header.size().next_multiple_of(ustar::BLOCK_SIZE);
        }
        None
    }

    pub fn file_start(header: &ustar::Header) -> *const [u8] {
        let start = unsafe {
            (header as *const ustar::Header)
                .byte_add(ustar::BLOCK_SIZE)
                .cast()
        };
        let size = header.size();
        ptr::slice_from_raw_parts(start, size)
    }
}
