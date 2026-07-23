// kernel/src/sync/sleeplock.rs
use crate::sync::spinlock::SpinLock;
use crate::proc::{sleep, wakeup, current_process, current_process_opt, started};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};

pub struct SleepLock<T> {
    locked: UnsafeCell<bool>,
    pid: UnsafeCell<usize>,
    data: UnsafeCell<T>,
    // Internal spinlock to protect the locked flag
    guard_lock: SpinLock<()>,
}

unsafe impl<T> Sync for SleepLock<T> where T: Send {}
unsafe impl<T> Send for SleepLock<T> where T: Send {}

impl<T> SleepLock<T> {
    pub const fn new(data: T, _name: &'static str) -> Self {
        Self { 
            locked: UnsafeCell::new(false), 
            pid: UnsafeCell::new(0), 
            data: UnsafeCell::new(data),
            guard_lock: SpinLock::new((), "sleeplock_guard"),
        }
    }
    
    pub fn acquire(&self) -> SleepLockGuard<'_, T> {
        if crate::proc::started() {
            let p = current_process();
            loop {
                // Acquire the internal guard lock
                let guard = self.guard_lock.acquire();
                if !unsafe { *self.locked.get() } {
                    unsafe { *self.locked.get() = true; }
                    unsafe { *self.pid.get() = p.pid(); }
                    drop(guard);
                    return SleepLockGuard { lock: self };
                }
                // Release the guard lock and sleep on this sleeplock's address
                drop(guard);
                // Sleep on this sleeplock's address as the channel
                sleep(self as *const _ as usize, &self.guard_lock);
                // After wakeup, loop will re-acquire the guard lock
            }
        } else {
            // Before scheduler starts: spin without sleeping
            loop {
                let guard = self.guard_lock.acquire();
                if !unsafe { *self.locked.get() } {
                    unsafe { *self.locked.get() = true; }
                    unsafe { *self.pid.get() = 0; }
                    drop(guard);
                    return SleepLockGuard { lock: self };
                }
                drop(guard);
                core::hint::spin_loop();
            }
        }
    }
    
    pub fn release(&self) {
        let _guard = self.guard_lock.acquire();
        unsafe { *self.locked.get() = false; }
        unsafe { *self.pid.get() = 0; }
        wakeup(self as *const _ as usize);
    }
    
    pub fn holding(&self) -> bool {
        let _guard = self.guard_lock.acquire();
        unsafe { *self.locked.get() }
    }
}

pub struct SleepLockGuard<'a, T> {
    lock: &'a SleepLock<T>,
}

impl<'a, T> Drop for SleepLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

impl<'a, T> Deref for SleepLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SleepLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}