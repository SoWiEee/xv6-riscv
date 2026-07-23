// kernel/src/sync/condvar.rs
use crate::sync::spinlock::{SpinLock, release_raw};
use crate::proc::process::{Proc, ProcState};
use crate::proc::current_process;
use alloc::collections::VecDeque;

pub struct Condvar {
    wait_queue: SpinLock<VecDeque<usize>>,
}

impl Condvar {
    pub const fn new(_name: &'static str) -> Self {
        Self { wait_queue: SpinLock::new(VecDeque::new(), "condvar") }
    }
    
    pub fn sleep(&self, lock: &SpinLock<impl Sized>) {
        let p = current_process();
        let mut q = self.wait_queue.acquire();
        q.push_back(p as *const Proc as usize);
        p.set_state(ProcState::Sleeping);
        // Release the wait queue lock before releasing the external lock
        drop(q);
        // Release the external lock while sleeping
        unsafe {
            release_raw(&lock.locked);
        }
        // Schedule
        crate::proc::sched();
        // Re-acquire external lock after wakeup
        lock.acquire();
    }
    
    pub fn wakeup(&self) {
        let mut q = self.wait_queue.acquire();
        while let Some(p_ptr) = q.pop_front() {
            let p = unsafe { &mut *(p_ptr as *mut Proc) };
            p.set_state(ProcState::Runnable);
        }
    }
    
    pub fn wakeup_one(&self) {
        let mut q = self.wait_queue.acquire();
        if let Some(p_ptr) = q.pop_front() {
            let p = unsafe { &mut *(p_ptr as *mut Proc) };
            p.set_state(ProcState::Runnable);
        }
    }
}

/// Global sleep/wakeup using channel pointers (like xv6)
pub fn sleep(_chan: usize, lock: &SpinLock<impl Sized>) {
    let _p = current_process();
    // Add to global wait queue keyed by chan
    // ... implementation using global HashMap<usize, Condvar>
    unsafe {
        release_raw(&lock.locked);
    }
    crate::proc::sched();
    lock.acquire();
}

pub fn wakeup(_chan: usize) {
    // Look up condvar for chan, call wakeup()
}