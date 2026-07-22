// kernel/src/sync/mutex.rs
use crate::sync::spinlock::SpinLock;

pub struct Mutex<T> {
    lock: SpinLock<Option<T>>,
}

impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self {
        Self {
            lock: SpinLock::new(Some(value), "mutex"),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        let guard = self.lock.acquire();
        MutexGuard { inner: guard }
    }

    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.lock.try_acquire().map(|guard| MutexGuard { inner: guard })
    }
}

pub struct MutexGuard<'a, T> {
    inner: crate::sync::spinlock::SpinLockGuard<'a, Option<T>>,
}

impl<'a, T> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner.as_ref().expect("MutexGuard: value is None")
    }
}

impl<'a, T> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.as_mut().expect("MutexGuard: value is None")
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        // Value is dropped when guard is dropped
    }
}