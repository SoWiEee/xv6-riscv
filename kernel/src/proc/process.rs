// kernel/src/proc/process.rs
use crate::mm::page_table::PageTable;
use crate::arch::trap::TrapFrame;
use crate::arch::trap::Context;
use crate::sync::spinlock::SpinLock;
use crate::fs::{Inode, File};
use alloc::vec::Vec;
use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcState {
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

pub const NPROC: usize = 64;
pub const NOFILE: usize = 16;

pub struct Proc {
    pub lock: SpinLock<ProcInner>,
}

// We need to implement Send/Sync for Proc since it contains raw pointers
// In a kernel, we manage synchronization manually with spinlocks
unsafe impl Send for Proc {}
unsafe impl Sync for Proc {}

pub struct ProcInner {
    pub state: ProcState,
    pub chan: usize,
    pub killed: bool,
    pub xstate: i32,
    pub pid: usize,
    pub parent: Option<*mut Proc>,
    pub kstack: usize,
    pub sz: usize,
    pub pagetable: Option<PageTable>,
    pub trapframe: *mut TrapFrame,
    pub context: Context,
    pub ofile: [Option<File>; NOFILE],
    pub cwd: Option<&'static Inode>,
    pub name: [u8; 16],
}

impl Proc {
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
    
    pub fn lock(&self) -> crate::sync::spinlock::SpinLockGuard<'_, ProcInner> {
        self.lock.acquire()
    }
    
    // Helper methods for common operations
    pub fn pid(&self) -> usize { 
        let inner = self.lock();
        inner.pid 
    }
    
    pub fn state(&self) -> ProcState { 
        let inner = self.lock();
        inner.state 
    }
    
    pub fn is_killed(&self) -> bool { 
        let inner = self.lock();
        inner.killed 
    }
    
    pub fn kill(&self) { 
        self.lock().killed = true; 
    }
    
    pub fn set_state(&self, state: ProcState) { 
        self.lock().state = state; 
    }
    
    pub fn set_chan(&self, chan: usize) { 
        self.lock().chan = chan; 
    }
    
    pub fn chan(&self) -> usize { 
        let inner = self.lock();
        inner.chan 
    }
    
    pub fn set_xstate(&self, xstate: i32) { 
        self.lock().xstate = xstate; 
    }
    
    pub fn xstate(&self) -> i32 { 
        let inner = self.lock();
        inner.xstate 
    }
    
    pub fn set_parent(&self, parent: *mut Proc) { 
        self.lock().parent = Some(parent); 
    }
    
    pub fn parent(&self) -> Option<*mut Proc> { 
        let inner = self.lock();
        inner.parent 
    }
    
    pub fn kstack(&self) -> usize { 
        let inner = self.lock();
        inner.kstack 
    }
    
    pub fn set_kstack(&self, kstack: usize) { 
        self.lock().kstack = kstack; 
    }
    
    pub fn sz(&self) -> usize { 
        let inner = self.lock();
        inner.sz 
    }
    
    pub fn set_sz(&self, sz: usize) { 
        self.lock().sz = sz; 
    }
    
    pub fn set_pagetable(&self, pt: PageTable) { 
        self.lock().pagetable = Some(pt); 
    }
    
    pub fn set_trapframe(&self, tf: *mut TrapFrame) { 
        self.lock().trapframe = tf; 
    }
    
    pub fn set_name(&self, name: &[u8]) { 
        let mut inner = self.lock();
        let len = core::cmp::min(name.len(), 16);
        inner.name[..len].copy_from_slice(&name[..len]);
    }
    
    pub fn set_pid(&self, pid: usize) { 
        self.lock().pid = pid; 
    }
    
    pub fn set_used(&self) { 
        self.lock().state = ProcState::Used; 
    }
    
    pub fn set_unused(&self) { 
        self.lock().state = ProcState::Unused; 
    }
    
    pub fn set_runnable(&self) { 
        self.lock().state = ProcState::Runnable; 
    }
    
    pub fn set_running(&self) { 
        self.lock().state = ProcState::Running; 
    }
    
    pub fn set_sleeping(&self) { 
        self.lock().state = ProcState::Sleeping; 
    }
    
    pub fn set_zombie(&self) { 
        self.lock().state = ProcState::Zombie; 
    }
}