#![deny(missing_docs, warnings)]
#![feature(unsafe_destructor, plugin)]

//! A mutex which can only be locked once, but which provides
//! very fast concurrent reads after the first lock is over.
//!
//! ## Example
//!
//! ```
//! # use oncemutex::OnceMutex;
//!
//! let mutex = OnceMutex::new(8u);
//!
//! // One-time lock
//! *mutex.lock().unwrap() = 9u;
//!
//! // Cheap lock-free access.
//! assert_eq!(*mutex, 9u);
//! ```
//!

#[cfg(test)] #[plugin]
extern crate stainless;
#[cfg(test)]
extern crate test;

use std::sync::{Mutex, MutexGuard};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::mem;

const UNUSED: usize = 0;
const LOCKED: usize = 1;
const FREE: usize = 2;

/// A mutex which can only be locked once, but which provides
/// very fast, lock-free, concurrent reads after the first
/// lock is over.
pub struct OnceMutex<T> {
    lock: Mutex<()>,
    state: AtomicUsize,
    data: UnsafeCell<T>
}

impl<T: Send + Sync> OnceMutex<T> {
    /// Create a new OnceMutex.
    pub fn new(x: T) -> OnceMutex<T> {
        OnceMutex {
            lock: Mutex::new(()),
            state: AtomicUsize::new(UNUSED),
            data: UnsafeCell::new(x)
        }
    }

    /// Attempt to lock the OnceMutex.
    ///
    /// This will not block, but will return None if the OnceMutex
    /// has already been locked or is currently locked by another thread.
    pub fn lock(&self) -> Option<OnceMutexGuard<T>> {
        match self.state.compare_and_swap(UNUSED, LOCKED, SeqCst) {
            // self.state is now LOCKED.
            UNUSED => {
                // Locks self.lock
                Some(OnceMutexGuard::new(self))
            },

            // Other thread got here first or already locked.
            // Either way, no lock.
            _ => None
        }
    }

    /// Block the current task until the first lock is over.
    ///
    /// Does nothing if there is no lock.
    pub fn wait(&self) {
        // Don't take out a lock if we aren't locked.
        if self.locked() { let _ = self.lock.lock(); }
    }

    /// Extract the data from a OnceMutex.
    pub fn into_inner(self) -> T {
        self.data.value
    }

    /// Is this OnceMutex currently locked?
    pub fn locked(&self) -> bool {
        self.state.load(SeqCst) == LOCKED
    }
}

impl<T: Send + Sync> Deref for OnceMutex<T> {
    type Target = T;

    /// Get a reference to the value inside the OnceMutex.
    ///
    /// This can block if the OnceMutex is in its lock, but is
    /// very fast otherwise.
    fn deref(&self) -> &T {
        if LOCKED == self.state.compare_and_swap(UNUSED, FREE, SeqCst) {
            // The OnceMutexGuard has not released yet.
            self.wait();
        }

        debug_assert_eq!(self.state.load(SeqCst), FREE);

        // We are FREE, so go!
        unsafe { mem::transmute(self.data.get()) }
    }
}

// Safe, because we have &mut self, which means no OnceMutexGuard's exist.
impl<T: Send + Sync> DerefMut for OnceMutex<T> {
    fn deref_mut(&mut self) -> &mut T {
        // Should be impossible.
        debug_assert!(self.state.load(SeqCst) != LOCKED);

        unsafe { mem::transmute(self.data.get()) }
    }
}

/// A guard providing a one-time lock on a OnceMutex.
pub struct OnceMutexGuard<'a, T: 'a> {
    parent: &'a OnceMutex<T>,
    // Only used for its existence, so triggers dead_code warnings.
    _lock: MutexGuard<'a, ()>
}

impl<'a, T> OnceMutexGuard<'a, T> {
    fn new(mutex: &'a OnceMutex<T>) -> OnceMutexGuard<'a, T> {
        OnceMutexGuard {
            parent: mutex,
            _lock: mutex.lock.lock().unwrap()
        }
    }
}

impl<'a, T: Send + Sync> DerefMut for OnceMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { mem::transmute(self.parent.data.get()) }
    }
}

impl<'a, T: Send + Sync> Deref for OnceMutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { mem::transmute(self.parent.data.get()) }
    }
}

#[unsafe_destructor]
impl<'a, T> Drop for OnceMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.parent.state.store(FREE, SeqCst);
    }
}

#[cfg(test)]
mod tests {
    pub use super::{OnceMutex, FREE, UNUSED, LOCKED};
    pub use std::sync::atomic::Ordering::{Relaxed, SeqCst};
    pub use std::sync::Mutex;

    describe! oncemutex {
        before_each {
            let mutex = OnceMutex::new("hello");
        }

        it "should lock once and only once" {
            assert!(mutex.lock().is_some());
            assert!(mutex.lock().is_none());
            assert!(mutex.lock().is_none());
        }

        it "should start with a state of UNUSED" {
            assert_eq!(mutex.state.load(Relaxed), UNUSED);
        }

        it "should set the state to LOCKED while locked" {
            let _lock = mutex.lock();
            assert_eq!(mutex.state.load(Relaxed), LOCKED);
        }

        it "should set the state to FREE when the lock drops" {
            drop(mutex.lock());
            assert_eq!(mutex.state.load(Relaxed), FREE);
        }

        it "should set the state to FREE when derefed" {
            *mutex;
            assert_eq!(mutex.state.load(Relaxed), FREE);
        }

        bench "locking" (bencher) {
            let mutex = OnceMutex::new(5u);
            bencher.iter(|| {
                ::test::black_box(mutex.lock());
                mutex.state.store(UNUSED, Relaxed);
            });
        }

        bench "access" (bencher) {
            let mutex = OnceMutex::new(5u);
            bencher.iter(|| ::test::black_box(*mutex));
        }
    }

    describe! mutex {
        bench "locking" (bencher) {
            let mutex = Mutex::new(5u);
            bencher.iter(|| ::test::black_box(mutex.lock()));
        }
    }
}

