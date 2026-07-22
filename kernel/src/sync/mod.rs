// kernel/src/sync/mod.rs
pub mod spinlock;
pub mod sleeplock;
pub mod condvar;

pub use spinlock::{SpinLock, SpinLockGuard, push_off, pop_off};
pub use sleeplock::{SleepLock, SleepLockGuard};
pub use condvar::{Condvar, sleep as cv_sleep, wakeup as cv_wakeup};