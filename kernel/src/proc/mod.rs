// kernel/src/proc/mod.rs
use crate::arch::trap::TrapFrame;
use crate::arch::trap::Context;
use crate::arch::asm::r_tp;
use crate::mm::address::PhysPageNum;
use crate::sync::spinlock::{SpinLock, release_raw};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::AtomicUsize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

pub struct Process {
    pub pid: usize,
    pub trapframe: TrapFrame,
    pub context: Context,
    pub pagetable: PhysPageNum,
    pub killed: bool,
    pub sz: usize,
    pub state: ProcState,
    pub chan: usize,  // Channel for sleep/wakeup
}

pub struct Cpu {
    pub noff: usize,      // Depth of push_off() nesting
    pub intena: bool,     // Were interrupts enabled before push_off()?
}

static mut CPUS: [Cpu; 8] = [const { Cpu { noff: 0, intena: false } }; 8];

pub fn mycpu() -> &'static mut Cpu {
    let hartid = r_tp();
    unsafe { &mut CPUS[hartid] }
}

static mut PROCESSES: [Option<Process>; 64] = [const { None }; 64];
static mut CURRENT_PROC: *mut Process = core::ptr::null_mut();
static mut NPROC: usize = 0;
static mut TICKS: usize = 0;

static NEXT_PID: AtomicUsize = AtomicUsize::new(1);

// Wait queues for sleep/wakeup - using usize (process pointer as usize) to avoid Send issues
static WAIT_QUEUES: SpinLock<BTreeMap<usize, Vec<usize>>> = 
    SpinLock::new(BTreeMap::new(), "wait_queues");

pub fn procinit() {
    unsafe {
        NPROC = 0;
        CURRENT_PROC = core::ptr::null_mut();
    }
}

pub fn current_process() -> &'static mut Process {
    unsafe {
        if CURRENT_PROC.is_null() {
            panic!("current_process: no current process");
        }
        &mut *CURRENT_PROC
    }
}

pub fn current_process_opt() -> Option<&'static mut Process> {
    unsafe {
        if CURRENT_PROC.is_null() {
            None
        } else {
            Some(&mut *CURRENT_PROC)
        }
    }
}

pub fn is_killed(p: &Process) -> bool {
    p.killed
}

pub fn set_killed(p: &mut Process) {
    p.killed = true;
}

pub fn kexit(code: i32) -> ! {
    panic!("kexit: {}", code);
}

pub fn yield_now() {
    // TODO: implement context switch
    loop {
        crate::arch::asm::wfi();
    }
}

pub fn tick() {
    unsafe {
        TICKS += 1;
    }
}

pub fn sched() {
    // TODO: implement scheduler
    loop {
        crate::arch::asm::wfi();
    }
}

pub fn scheduler() -> ! {
    loop {
        crate::arch::asm::wfi();
    }
}

pub fn userinit() {
    // Create first user process
    crate::arch::console::printk(format_args!("userinit: creating first user process\n"));
}

pub fn started() -> bool {
    unsafe { NPROC > 0 }
}

/// Sleep on a channel, releasing the given lock.
/// The lock must be held before calling sleep.
/// This matches xv6's sleep(chan, lock) signature.
/// Note: The caller must hold the lock (have called acquire) but NOT hold a SpinLockGuard.
/// This function will release the lock, sleep, and re-acquire it.
pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = current_process();
    let mut queues = WAIT_QUEUES.acquire();
    queues.entry(chan).or_default().push(p as *const Process as usize);
    p.state = ProcState::Sleeping;
    p.chan = chan;
    // Release the lock by manually unlocking
    // SAFETY: The caller guarantees the lock is held but no guard exists
    unsafe {
        release_raw(&lock.locked);
    }
    sched();
    // Re-acquire the lock
    lock.acquire();
    // Remove from wait queue after wakeup
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        vec.retain(|&ptr| ptr != p as *const Process as usize);
    }
}

/// Wake up all processes sleeping on a channel.
pub fn wakeup(chan: usize) {
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        let ptrs: Vec<usize> = vec.drain(..).collect();
        for p_ptr in ptrs {
            let p = unsafe { &mut *(p_ptr as *mut Process) };
            p.state = ProcState::Runnable;
        }
    }
}