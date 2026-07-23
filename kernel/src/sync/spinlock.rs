// kernel/src/sync/spinlock.rs
//! Spinlock with interrupt disable for mutual exclusion in interrupt contexts.
//!
//! This is the primary synchronization primitive for kernel code that may be
//! called from interrupt handlers. It disables interrupts on the local CPU
//! while held to prevent deadlocks.

use crate::arch::asm::{intr_on, intr_off, intr_get, r_tp};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// A mutual exclusion lock that disables interrupts while held.
/// 
/// Uses a simple spin-wait with atomic compare-and-swap. When acquired,
/// interrupts are disabled on the current CPU (via `push_off`/`pop_off`)
/// to prevent deadlock if an interrupt handler tries to acquire the same lock.
/// 
/// # Type Parameters
/// * `T` - The data protected by this lock. Must be `Send`.
/// 
/// # Example
/// ```
/// let lock = SpinLock::new(0, "counter");
/// {
///     let mut guard = lock.acquire();
///     *guard += 1;
/// } // Lock released automatically here
/// ```
pub struct SpinLock<T> {
    /// Atomic flag indicating if lock is held.
    pub locked: AtomicBool,
    /// Lock name for debugging.
    pub name: &'static str,
    data: UnsafeCell<T>,
    cpu: UnsafeCell<usize>,   // For debugging: which CPU holds the lock
}

unsafe impl<T> Sync for SpinLock<T> where T: Send {}
unsafe impl<T> Send for SpinLock<T> where T: Send {}

impl<T> SpinLock<T> {
    /// Create a new spinlock protecting `data` with the given `name`.
    pub const fn new(data: T, name: &'static str) -> Self {
        Self {
            locked: AtomicBool::new(false),
            name,
            data: UnsafeCell::new(data),
            cpu: UnsafeCell::new(0),
        }
    }
    
    /// Acquire the lock, spinning until available.
    /// 
    /// Disables interrupts on the current CPU. Returns a guard that releases
    /// the lock and restores interrupts when dropped.
    pub fn acquire(&self) -> SpinLockGuard<'_, T> {
        push_off();
        while self.locked.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        unsafe { *self.cpu.get() = r_tp(); }
        SpinLockGuard { lock: self }
    }
    
    /// Try to acquire the lock without spinning.
    /// 
    /// Returns `Some(guard)` if successful, `None` if lock is held.
    /// Disables interrupts on success.
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
    
    /// Check if the current CPU holds this lock.
    ///
    /// Used for debugging assertions.
    pub fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) && unsafe { *self.cpu.get() } == r_tp()
    }

    /// Raw pointer to the protected data. Only sound to dereference while the
    /// lock is held. Used by the scheduler, which keeps a proc lock held across
    /// a context switch (so no live guard exists to deref through).
    pub fn data_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// Release a lock that is held on the current CPU WITHOUT a live guard.
    ///
    /// Mirrors `SpinLockGuard::drop` exactly (clear owner, clear flag, `pop_off`).
    /// This is the counterpart to holding a lock across a context switch via
    /// `core::mem::forget(guard)`: the frame that acquired the lock is frozen
    /// during the switch, so the far side of the switch releases it here.
    ///
    /// # Safety
    /// The lock MUST currently be held by this CPU (e.g. acquired with a guard
    /// that was then `forget`-ten), with the matching `push_off` still in effect.
    pub unsafe fn raw_release(&self) {
        unsafe { *self.cpu.get() = 0; }
        self.locked.store(false, Ordering::Release);
        pop_off();
    }
}

/// RAII guard that releases the spinlock on drop.
/// 
/// Implements `Deref` and `DerefMut` for transparent access to protected data.
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

/// Disable interrupts and increment nesting counter.
/// 
/// Called by `acquire()` and `try_acquire()`. Must be paired with `pop_off()`.
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

/// Decrement nesting counter and restore interrupts if zero.
/// 
/// Called by `SpinLockGuard::drop()` and `release_raw()`.
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

/// Release a spinlock without a guard (for sleep/wakeup).
/// 
/// # Safety
/// Caller must ensure the lock is actually held by the current CPU
/// and that interrupts were disabled via `push_off()`.
pub unsafe fn release_raw(lock: &AtomicBool) {
    lock.store(false, Ordering::Release);
    pop_off();
}