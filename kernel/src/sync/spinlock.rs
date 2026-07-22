// kernel/src/sync/spinlock.rs
use crate::arch::asm::{intr_on, intr_off, intr_get, r_tp};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock<T> {
    pub locked: AtomicBool,
    pub name: &'static str,
    data: UnsafeCell<T>,
    cpu: UnsafeCell<usize>,   // For debugging: which CPU holds the lock
}

unsafe impl<T> Sync for SpinLock<T> where T: Send {}
unsafe impl<T> Send for SpinLock<T> where T: Send {}

impl<T> SpinLock<T> {
    pub const fn new(data: T, name: &'static str) -> Self {
        Self {
            locked: AtomicBool::new(false),
            name,
            data: UnsafeCell::new(data),
            cpu: UnsafeCell::new(0),
        }
    }
    
    pub fn acquire(&self) -> SpinLockGuard<'_, T> {
        push_off();
        while self.locked.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        unsafe { *self.cpu.get() = r_tp(); }
        SpinLockGuard { lock: self }
    }
    
    pub fn try_acquire(&self) -> Option<SpinLockGuard<'_, T>> {
        push_off();
        if self.locked.swap(true, Ordering::Acquire) {
            pop_off();
            None
        } else {
            unsafe { *self.cpu.get() = r_tp(); }
            Some(SpinLockGuard { lock: self })
        }
    }
    
    pub fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) && unsafe { *self.cpu.get() } == r_tp()
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        unsafe { *self.lock.cpu.get() = 0; }
        self.lock.locked.store(false, Ordering::Release);
        pop_off();
    }
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

/// Push interrupt disable nesting
pub fn push_off() {
    let intr = intr_get();
    intr_off();
    // Store previous state in per-CPU variable
    let cpu = crate::proc::mycpu();
    cpu.noff += 1;
    if cpu.noff == 1 {
        cpu.intena = intr;
    }
}

/// Pop interrupt disable nesting
pub fn pop_off() {
    let cpu = crate::proc::mycpu();
    if cpu.noff == 0 {
        panic!("pop_off: noff == 0");
    }
    cpu.noff -= 1;
    if cpu.noff == 0 && cpu.intena {
        intr_on();
    }
}

/// Release a spinlock without a guard (for sleep/wakeup)
/// SAFETY: Caller must ensure the lock is actually held
pub unsafe fn release_raw(lock: &AtomicBool) {
    lock.store(false, Ordering::Release);
    pop_off();
}