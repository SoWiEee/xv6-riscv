// kernel/src/proc/scheduler.rs
use crate::proc::process::{Proc, ProcState, NPROC, NOFILE};
use crate::arch::trap::{TrapFrame, Context};
use crate::arch::asm::{intr_on, intr_off, intr_get, w_satp, make_satp, r_tp};
use crate::mm::page_table::{PageTable, kernel_pagetable, uvmcreate, uvmalloc, uvmfree, uvmcopy};
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::sync::spinlock::SpinLock;
use crate::sync::mutex::Mutex;
use alloc::vec::Vec;

pub static PROCS: [Proc; NPROC] = [const { Proc::new() }; NPROC];
pub static mut SCHEDULER_STARTED: bool = false;

static NEXT_PID: Mutex<usize> = Mutex::new(1);

fn next_pid() -> usize {
    let mut pid = NEXT_PID.lock();
    *pid += 1;
    *pid
}

fn alloc_kernel_stack() -> usize {
    let page = alloc_page().expect("alloc_kernel_stack: out of memory");
    page.to_paddr().0
}

fn free_kernel_stack(kstack: usize) {
    if kstack != 0 {
        let ppn = crate::mm::address::PhysPageNum::new(kstack >> 12);
        free_page(ppn);
    }
}

pub fn procinit() {
    // Initialize process table
    for (i, p) in PROCS.iter().enumerate() {
        let mut inner = p.lock();
        inner.pid = 0; // Will be set on alloc
    }
}

pub fn alloc_proc() -> Option<&'static Proc> {
    for p in &PROCS {
        let mut inner = p.lock();
        if inner.state == ProcState::Unused {
            inner.state = ProcState::Used;
            inner.pid = next_pid();
            inner.kstack = alloc_kernel_stack();
            inner.trapframe = (inner.kstack + 4096 - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
            inner.context = Context::new();
            inner.name = [0; 16];
            inner.ofile = [const { None }; NOFILE];
            inner.cwd = None;
            inner.sz = 0;
            inner.pagetable = None;
            inner.chan = 0;
            inner.killed = false;
            inner.xstate = 0;
            inner.parent = None;
            return Some(p);
        }
    }
    None
}

pub fn free_proc(p: &Proc) {
    let mut inner = p.lock();
    inner.state = ProcState::Unused;
    inner.pid = 0;
    if inner.kstack != 0 {
        free_kernel_stack(inner.kstack);
        inner.kstack = 0;
    }
    if let Some(pt) = inner.pagetable.take() {
        drop(pt);
    }
    inner.trapframe = core::ptr::null_mut();
    inner.context = Context::new();
    inner.name = [0; 16];
    inner.ofile = [const { None }; NOFILE];
    inner.cwd = None;
    inner.sz = 0;
    inner.chan = 0;
    inner.killed = false;
    inner.xstate = 0;
    inner.parent = None;
}

pub fn scheduler() -> ! {
    unsafe { SCHEDULER_STARTED = true; }
    intr_on();
    loop {
        let hart = r_tp();
        for p in &PROCS {
            let mut inner = p.lock();
            if inner.state == ProcState::Runnable {
                inner.state = ProcState::Running;
                
                // Set current process for this CPU
                let cpu = crate::proc::mycpu();
                cpu.proc = Some(p);
                
                // Switch to process's page table
                if let Some(pt) = &inner.pagetable {
                    let satp = make_satp(pt.root_ppn().0);
                    w_satp(satp);
                }
                
                drop(inner);
                
                // Context switch to the process
                crate::arch::trap::context_switch(
                    &mut cpu.context,
                    &p.lock().context
                );
                
                // After returning, we're back in kernel
                cpu.proc = None;
                w_satp(kernel_pagetable().0);
            }
        }
    }
}

pub fn yield_now() {
    let p = crate::proc::current_process();
    let mut inner = p.lock();
    inner.state = ProcState::Runnable;
    drop(inner);
    sched();
}

pub fn sched() {
    let p = crate::proc::current_process();
    let mut inner = p.lock();
    if intr_get() {
        panic!("sched: interrupts enabled");
    }
    if inner.state == ProcState::Running {
        panic!("sched: running");
    }
    let cpu = crate::proc::mycpu();
    crate::arch::trap::context_switch(&mut inner.context, &cpu.context);
}

pub fn scheduler_started() -> bool {
    unsafe { SCHEDULER_STARTED }
}