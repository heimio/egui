//! Helper module that wraps some Mutex types with different implementations.

// ----------------------------------------------------------------------------

// ----------------------------------------------------------------------------

mod mutex_impl {
    // `atomic_refcell` will panic if multiple threads try to access the same value

    /// Provides interior mutability.
    ///
    /// Uses `parking_lot` crate on native targets, and `atomic_refcell` on `wasm32` targets.
    #[derive(Default)]
    pub struct Mutex<T>(atomic_refcell::AtomicRefCell<T>);

    /// The lock you get from [`Mutex`].
    pub use atomic_refcell::AtomicRefMut as MutexGuard;

    impl<T> Mutex<T> {
        #[inline(always)]
        pub fn new(val: T) -> Self {
            Self(atomic_refcell::AtomicRefCell::new(val))
        }

        /// Panics if already locked.
        #[inline(always)]
        pub fn lock(&self) -> MutexGuard<'_, T> {
            self.0.borrow_mut()
        }
    }
}

mod rw_lock_impl {
    // `atomic_refcell` will panic if multiple threads try to access the same value

    /// The lock you get from [`RwLock::read`].
    pub use atomic_refcell::AtomicRef as RwLockReadGuard;

    /// The lock you get from [`RwLock::write`].
    pub use atomic_refcell::AtomicRefMut as RwLockWriteGuard;

    /// Provides interior mutability.
    ///
    /// Uses `parking_lot` crate on native targets, and `atomic_refcell` on `wasm32` targets.
    #[derive(Default)]
    pub struct RwLock<T>(atomic_refcell::AtomicRefCell<T>);

    impl<T> RwLock<T> {
        #[inline(always)]
        pub fn new(val: T) -> Self {
            Self(atomic_refcell::AtomicRefCell::new(val))
        }

        #[inline(always)]
        pub fn read(&self) -> RwLockReadGuard<'_, T> {
            self.0.borrow()
        }

        /// Panics if already locked.
        #[inline(always)]
        pub fn write(&self) -> RwLockWriteGuard<'_, T> {
            self.0.borrow_mut()
        }
    }
}

// ----------------------------------------------------------------------------

use std::cell::RefCell;
use std::ffi::c_void;
use std::thread::LocalKey;
pub use mutex_impl::{Mutex, MutexGuard};
pub use rw_lock_impl::{RwLock, RwLockReadGuard, RwLockWriteGuard};

impl<T> Clone for Mutex<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self::new(self.lock().clone())
    }
}

// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::mutex::Mutex;
    use std::time::Duration;

    #[test]
    fn lock_two_different_mutexes_single_thread() {
        let one = Mutex::new(());
        let two = Mutex::new(());
        let _a = one.lock();
        let _b = two.lock();
    }

    #[test]
    #[should_panic]
    fn lock_reentry_single_thread() {
        let one = Mutex::new(());
        let _a = one.lock();
        let _a2 = one.lock(); // panics
    }

    #[test]
    fn lock_multiple_threads() {
        use std::sync::Arc;
        let one = Arc::new(Mutex::new(()));
        let our_lock = one.lock();
        let other_thread = {
            let one = Arc::clone(&one);
            std::thread::spawn(move || {
                let _ = one.lock();
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        drop(our_lock);
        other_thread.join().unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(feature = "deadlock_detection")]
#[cfg(test)]
mod tests_rwlock {
    use crate::mutex::RwLock;
    use std::time::Duration;

    #[test]
    fn lock_two_different_rwlocks_single_thread() {
        let one = RwLock::new(());
        let two = RwLock::new(());
        let _a = one.write();
        let _b = two.write();
    }

    #[test]
    fn rwlock_multiple_threads() {
        use std::sync::Arc;
        let one = Arc::new(RwLock::new(()));
        let our_lock = one.write();
        let other_thread1 = {
            let one = Arc::clone(&one);
            std::thread::spawn(move || {
                let _ = one.write();
            })
        };
        let other_thread2 = {
            let one = Arc::clone(&one);
            std::thread::spawn(move || {
                let _ = one.read();
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        drop(our_lock);
        other_thread1.join().unwrap();
        other_thread2.join().unwrap();
    }

    #[test]
    #[should_panic]
    fn rwlock_write_write_reentrancy() {
        let one = RwLock::new(());
        let _a1 = one.write();
        let _a2 = one.write(); // panics
    }

    #[test]
    #[should_panic]
    fn rwlock_write_read_reentrancy() {
        let one = RwLock::new(());
        let _a1 = one.write();
        let _a2 = one.read(); // panics
    }

    #[test]
    #[should_panic]
    fn rwlock_read_write_reentrancy() {
        let one = RwLock::new(());
        let _a1 = one.read();
        let _a2 = one.write(); // panics
    }

    #[test]
    fn rwlock_read_read_reentrancy() {
        let one = RwLock::new(());
        let _a1 = one.read();
        // This is legal: this test suite specifically targets native, which relies
        // on parking_lot's rw-locks, which are reentrant.
        let _a2 = one.read();
    }

    #[test]
    fn rwlock_short_read_foreign_read_write_reentrancy() {
        use std::sync::Arc;

        let lock = Arc::new(RwLock::new(()));

        // Thread #0 grabs a read lock
        let t0r0 = lock.read();

        // Thread #1 grabs the same read lock
        let other_thread = {
            let lock = Arc::clone(&lock);
            std::thread::spawn(move || {
                let _t1r0 = lock.read();
            })
        };
        other_thread.join().unwrap();

        // Thread #0 releases its read lock
        drop(t0r0);

        // Thread #0 now grabs a write lock, which is legal
        let _t0w0 = lock.write();
    }

    #[test]
    #[should_panic]
    fn rwlock_read_foreign_read_write_reentrancy() {
        use std::sync::Arc;

        let lock = Arc::new(RwLock::new(()));

        // Thread #0 grabs a read lock
        let _t0r0 = lock.read();

        // Thread #1 grabs the same read lock
        let other_thread = {
            let lock = Arc::clone(&lock);
            std::thread::spawn(move || {
                let _t1r0 = lock.read();
            })
        };
        other_thread.join().unwrap();

        // Thread #0 now grabs a write lock, which should panic (read-write)
        let _t0w0 = lock.write(); // panics
    }
}
