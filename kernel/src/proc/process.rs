// kernel/src/proc/process.rs
//! Process management structures.
//!
//! Defines the process control block (PCB) and process state machine.

use crate::mm::page_table::PageTable;
use crate::arch::trap::TrapFrame;
use crate::arch::trap::Context;
use crate::sync::spinlock::SpinLock;
use crate::fs::{Inode, File};
use alloc::sync::Arc;

/// Process states in the lifecycle.
/// 
/// Transitions: Unused -> Used -> Runnable -> Running <-> Sleeping -> Runnable -> Zombie -> Unused
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcState {
    /// Process slot is free.
    Unused,
    /// Process allocated but not initialized.
    Used,
    /// Waiting on a channel (sleep).
    Sleeping,
    /// Ready to run, waiting for scheduler.
    Runnable,
    /// Currently executing on a CPU.
    Running,
    /// Exited, waiting for parent to wait().
    Zombie,
}

/// Maximum number of processes.
pub const NPROC: usize = 64;

/// Maximum number of open files per process.
pub const NOFILE: usize = 16;

/// Process control block.
/// 
/// All mutable state is protected by the inner spinlock. The `Proc` struct
/// itself is immutable and can be shared via `Arc<Proc>`.
pub struct Proc {
    /// Spinlock protecting all mutable fields.
    pub lock: SpinLock<ProcInner>,
}

// We need to implement Send/Sync for Proc since it contains raw pointers
// In a kernel, we manage synchronization manually with spinlocks
unsafe impl Send for Proc {}
unsafe impl Sync for Proc {}

/// Mutable process state protected by `Proc.lock`.
pub struct ProcInner {
    /// Current process state.
    pub state: ProcState,
    /// Wait channel for sleep/wakeup.
    pub chan: usize,
    /// True if process has been killed.
    pub killed: bool,
    /// Exit status for parent's wait().
    pub xstate: i32,
    /// Process ID.
    pub pid: usize,
    /// Parent process pointer (raw to avoid cycles).
    pub parent: Option<*mut Proc>,
    /// Kernel stack top virtual address.
    pub kstack: usize,
    /// User memory size (bytes).
    pub sz: usize,
    /// User page table (None for kernel threads).
    pub pagetable: Option<PageTable>,
    /// User trap frame (raw pointer for asm access).
    pub trapframe: *mut TrapFrame,
    /// Kernel context for switching.
    pub context: Context,
    /// Open file descriptors.
    pub ofile: [Option<Arc<File>>; NOFILE],
    /// Current working directory.
    pub cwd: Option<*const Inode>,
    /// Process name (for debugging).
    pub name: [u8; 16],
}

impl Proc {
    /// Create a new uninitialized process.
    pub const fn new() -> Self {
        Self {
            lock: SpinLock::new(ProcInner {
                state: ProcState::Unused,
                chan: 0,
                killed: false,
                xstate: 0,
                pid: 0,
                parent: None,
                kstack: 0,
                sz: 0,
                pagetable: None,
                trapframe: core::ptr::null_mut(),
                context: Context::new(),
                ofile: [const { None }; NOFILE],
                cwd: None,
                name: [0; 16],
            }, "proc")
        }
    }
    
    /// Acquire the process lock.
    /// 
    /// Returns a guard that releases the lock on drop.
    pub fn lock(&self) -> crate::sync::spinlock::SpinLockGuard<'_, ProcInner> {
        self.lock.acquire()
    }
    
    /// Get the process ID.
    pub fn pid(&self) -> usize { 
        let inner = self.lock();
        inner.pid 
    }
    
    /// Get the current process state.
    pub fn state(&self) -> ProcState { 
        let inner = self.lock();
        inner.state 
    }
    
    /// Check if the process has been killed.
    pub fn is_killed(&self) -> bool { 
        let inner = self.lock();
        inner.killed 
    }
    
    /// Mark the process as killed.
    pub fn kill(&self) { 
        self.lock().killed = true; 
    }
    
    /// Set the process state.
    pub fn set_state(&self, state: ProcState) { 
        self.lock().state = state; 
    }
    
    /// Set the wait channel.
    pub fn set_chan(&self, chan: usize) { 
        self.lock().chan = chan; 
    }
    
    /// Get the wait channel.
    pub fn chan(&self) -> usize { 
        let inner = self.lock();
        inner.chan 
    }
    
    /// Set the exit status.
    pub fn set_xstate(&self, xstate: i32) { 
        self.lock().xstate = xstate; 
    }
    
    /// Get the exit status.
    pub fn xstate(&self) -> i32 { 
        let inner = self.lock();
        inner.xstate 
    }
    
    /// Set the parent process.
    pub fn set_parent(&self, parent: *mut Proc) { 
        self.lock().parent = Some(parent); 
    }
    
    /// Get the parent process.
    pub fn parent(&self) -> Option<*mut Proc> { 
        let inner = self.lock();
        inner.parent 
    }
    
    /// Get the kernel stack address.
    pub fn kstack(&self) -> usize { 
        let inner = self.lock();
        inner.kstack 
    }
    
    /// Set the kernel stack address.
    pub fn set_kstack(&self, kstack: usize) { 
        self.lock().kstack = kstack; 
    }
    
    /// Get the user memory size.
    pub fn sz(&self) -> usize { 
        let inner = self.lock();
        inner.sz 
    }
    
    /// Set the user memory size.
    pub fn set_sz(&self, sz: usize) { 
        self.lock().sz = sz; 
    }
    
    /// Set the user page table.
    pub fn set_pagetable(&self, pt: PageTable) { 
        self.lock().pagetable = Some(pt); 
    }
    
    /// Set the user trap frame pointer.
    pub fn set_trapframe(&self, tf: *mut TrapFrame) { 
        self.lock().trapframe = tf; 
    }
    
    /// Set the process name (truncated to 16 bytes).
    pub fn set_name(&self, name: &[u8]) { 
        let mut inner = self.lock();
        let len = core::cmp::min(name.len(), 16);
        inner.name[..len].copy_from_slice(&name[..len]);
    }
    
    /// Set the process ID.
    pub fn set_pid(&self, pid: usize) { 
        self.lock().pid = pid; 
    }
    
    /// Mark process as Used (allocated but not initialized).
    pub fn set_used(&self) { 
        self.lock().state = ProcState::Used; 
    }
    
    /// Mark process as Unused (free slot).
    pub fn set_unused(&self) { 
        self.lock().state = ProcState::Unused; 
    }
    
    /// Mark process as Runnable.
    pub fn set_runnable(&self) { 
        self.lock().state = ProcState::Runnable; 
    }
    
    /// Mark process as Running.
    pub fn set_running(&self) { 
        self.lock().state = ProcState::Running; 
    }
    
    /// Mark process as Sleeping.
    pub fn set_sleeping(&self) { 
        self.lock().state = ProcState::Sleeping; 
    }
    
    /// Mark process as Zombie.
    pub fn set_zombie(&self) { 
        self.lock().state = ProcState::Zombie; 
    }
}