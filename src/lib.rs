#![license = "MIT"]
#![deny(missing_docs, warnings)]
#![feature(unsafe_destructor)]

//! Crate comment goes here

use std::sync::{Mutex, MutexGuard};
use std::sync::atomic::AtomicUint;
use std::sync::atomic::Ordering::SeqCst;
use std::cell::UnsafeCell;
use std::mem;

const UNUSED: uint = 0;
const LOCKED: uint = 1;
const FREE: uint = 2;

/// A mutex which can only be locked once, but which provides
/// very fast concurrent reads after the first lock is over.
pub struct OnceMutex<T> {
    lock: Mutex<()>,
    state: AtomicUint,
    data: UnsafeCell<T>
}

impl<T: Send + Sync> OnceMutex<T> {
    /// Create a new OnceMutex.
    pub fn new(x: T) -> OnceMutex<T> {
        OnceMutex {
            lock: Mutex::new(()),
            state: AtomicUint::new(UNUSED),
            data: UnsafeCell::new(x)
        }
    }

    /// Attempt to lock the OnceMutex.
    ///
    /// This will not block, but will return None if the OnceMutex
    /// has already been locked or is currently locked by another thread.
    pub fn lock(&self) -> Option<OnceMutexGuard<T>> {
        match self.state.compare_and_swap(UNUSED, LOCKED, SeqCst) {
            LOCKED => {
                // Locks self.lock
                Some(OnceMutexGuard::new(self))
            },

            // Other thread got here first or already locked.
            // Either way, no lock.
            _ => None
        }
    }

    /// Extract the data from a OnceMutex.
    pub fn into_inner(self) -> T {
        self.data.value
    }
}

impl<T: Send + Sync> Deref<T> for OnceMutex<T> {
    /// Get a reference to the value inside the OnceMutex.
    ///
    /// This can block if the OnceMutex is in its lock, but is
    /// very fast otherwise.
    fn deref(&self) -> &T {
        if LOCKED == self.state.compare_and_swap(UNUSED, FREE, SeqCst) {
            // self.lock is locked and the OnceMutexGuard has not released yet.
            //
            // Get a lock on self.lock, so we block until the OnceMutexGuard is gone.
            self.lock.lock();
        }

        debug_assert_eq!(self.state.load(SeqCst), FREE);

        // We are FREE, so go!
        unsafe { mem::transmute(self.data.get()) }
    }
}

/// A guard providing a one-time lock on a OnceMutex.
pub struct OnceMutexGuard<'a, T: 'a> {
    parent: &'a OnceMutex<T>,
    // Only used for its existence, so triggers dead_code warnings.
    _lock: MutexGuard<'a, ()>
}

impl<'a, T: Send + Sync> OnceMutexGuard<'a, T> {
    fn new(mutex: &'a OnceMutex<T>) -> OnceMutexGuard<'a, T> {
        OnceMutexGuard {
            parent: mutex,
            _lock: mutex.lock.lock()
        }
    }
}

impl<'a, T: Send + Sync> DerefMut<T> for OnceMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { mem::transmute(self.parent.data.get()) }
    }
}

impl<'a, T: Send + Sync> Deref<T> for OnceMutexGuard<'a, T> {
    fn deref(&self) -> &T {
        unsafe { mem::transmute(self.parent.data.get()) }
    }
}

#[unsafe_destructor]
impl<'a, T: Send + Sync> Drop for OnceMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.parent.state.store(FREE, SeqCst);
    }
}

