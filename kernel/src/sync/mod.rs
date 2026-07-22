// kernel/src/sync/mod.rs
pub mod spinlock;

pub use spinlock::{SpinLock, SpinLockGuard};