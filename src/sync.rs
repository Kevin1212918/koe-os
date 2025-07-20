use core::cell::SyncUnsafeCell;
use core::mem::MaybeUninit;
use core::ops::{Deref, DerefMut};

pub mod base;
pub mod spin;

unsafe impl<T> Sync for InitCell<T> where T: Sync {}
unsafe impl<T> Send for InitCell<T> where T: Send {}

/// A thin wrapper around [`MaybeUninit`] which shifts the safety requirement
/// from access to initialization.
///
/// Creating an `InitCell` is unsafe and requires any access to the cell to
/// occur after `init` is called.
#[derive(Debug)]
#[repr(transparent)]
pub struct InitCell<T>(SyncUnsafeCell<MaybeUninit<T>>);
impl<T> InitCell<T> {
    /// Create `InitCell`.
    ///
    /// # Safety
    /// Cell content should not be accessed before any of the init functions is
    /// called.
    pub const unsafe fn new() -> Self {
        Self(SyncUnsafeCell::new(
            MaybeUninit::uninit(),
        ))
    }

    /// Initialize the cell with given value.
    ///
    /// # Safety
    /// - init functions are not thread safe, and so requires external
    ///   synchronization.
    /// - init functions can be called at most once.
    pub unsafe fn init(&self, val: T) -> &T {
        unsafe { self.0.get().as_mut_unchecked().write(val) }
    }

    /// Initialize the cell with a closure.
    ///
    /// # Safety
    /// See [`InitCell::init`] for safety requirement.
    pub unsafe fn init_with(&self, f: fn() -> T) -> &T { unsafe { self.init(f()) } }
}

impl<T> Deref for InitCell<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: safety follows from the requirements on init and new.
        unsafe { self.0.get().as_ref_unchecked().assume_init_ref() }
    }
}

impl<T> DerefMut for InitCell<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: safety follows from the requirements on init and new.
        unsafe { self.0.get().as_mut_unchecked().assume_init_mut() }
    }
}
