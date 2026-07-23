// kernel/src/proc/scheduler.rs
use crate::proc::process::{Proc, ProcState, NPROC, NOFILE};
use crate::arch::trap::{TrapFrame, Context};
use crate::arch::asm::{intr_on, intr_off, intr_get, w_satp, make_satp, r_tp, r_sstatus, w_sstatus, w_sepc};
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
    crate::arch::console::printk(format_args!("procinit: start\n"));
    let ptr = unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR };
    crate::arch::console::printk(format_args!("procinit: ptr={:#x}\n", ptr as usize));
    // Initialize process table
    for (i, p) in PROCS.iter().enumerate() {
        let mut inner = p.lock();
        inner.pid = 0; // Will be set on alloc
    }
    let ptr = unsafe { crate::mm::page_table::KERNEL_PAGETABLE_PTR };
    crate::arch::console::printk(format_args!("procinit: end ptr={:#x}\n", ptr as usize));
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
    crate::proc::set_scheduler_started();
    crate::arch::console::printk(format_args!("scheduler: started\n"));
    intr_on();
    loop {
        let hart = r_tp();
        for p in &PROCS {
            let mut inner = p.lock();
            if inner.state == ProcState::Runnable {
                crate::arch::console::printk(format_args!("scheduler: found runnable pid={}\n", inner.pid));
                inner.state = ProcState::Running;
                
                // Set current process for this CPU while holding lock
                let cpu = crate::proc::mycpu();
                cpu.proc = Some(p);
                
                // Switch to process's page table
                if let Some(pt) = &inner.pagetable {
                    let satp = make_satp(pt.root_ppn().0);
                    w_satp(satp);
                }
                
                // Prepare return to user mode: set sstatus.SPP=0, SPIE=1
                {
                    let mut sstatus = crate::arch::asm::r_sstatus();
                    sstatus &= !0x100; // clear SPP (bit 8)
                    sstatus |= 0x20;   // set SPIE (bit 5)
                    crate::arch::asm::w_sstatus(sstatus);
                    // Set sepc from trapframe
                    let tf = unsafe { &*inner.trapframe };
                    crate::arch::asm::w_sepc(tf.epc);
                    // Set sscratch to trapframe pointer for uservec
                    crate::arch::asm::w_sscratch(inner.trapframe as usize);
                }
                
                // Save context pointer and trapframe pointer before dropping lock
                let ctx_ptr = &mut inner.context as *mut _;
                let tf_ptr = inner.trapframe as usize;
                
                drop(inner);
                
                // Context switch to the process
                crate::arch::trap::context_switch(
                    &mut cpu.context,
                    unsafe { &mut *ctx_ptr },
                    tf_ptr
                );
                
                // After returning, we're back in kernel
                let cpu = crate::proc::mycpu();
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
    crate::arch::trap::context_switch(&mut inner.context, &cpu.context, 0);
}

pub fn scheduler_started() -> bool {
    unsafe { SCHEDULER_STARTED }
}