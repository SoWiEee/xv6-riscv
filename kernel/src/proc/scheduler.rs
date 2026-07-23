// kernel/src/proc/scheduler.rs
use crate::proc::process::{Proc, ProcState, NPROC, NOFILE};
use crate::arch::trap::{TrapFrame, Context};
use crate::arch::asm::{intr_on, intr_off, intr_get, w_satp, make_satp, r_tp, r_sstatus, w_sstatus, w_sepc, w_stvec, TRAMPOLINE};
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
            // The trapframe needs its own page-aligned page so it can be mapped
            // at the fixed TRAPFRAME virtual address in the user page table.
            inner.trapframe = alloc_page()
                .expect("alloc_proc: out of memory for trapframe")
                .to_paddr().0 as *mut TrapFrame;
            // Fresh frames from the allocator are not zeroed; a garbage trapframe
            // would be restored into user registers (e.g. gp/tp) by userret.
            unsafe { *inner.trapframe = TrapFrame::new(); }
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
                
                // Set current process for this CPU while holding the lock.
                let cpu = crate::proc::mycpu();
                cpu.proc = Some(p);

                // Do NOT touch satp/sstatus/sepc/stvec here. On a process's
                // first run new.ra = forkret, which calls usertrapret; that is
                // what programs the return-to-user CSRs and switches to the user
                // page table (inside the trampoline `userret`). The scheduler
                // itself runs entirely under the kernel page table.
                let ctx_ptr = &mut inner.context as *mut Context;
                drop(inner);

                // Switch into the process. Control returns here when the process
                // switches back out (via sched()), still on the kernel page table.
                crate::arch::trap::context_switch(
                    &mut cpu.context,
                    unsafe { &*ctx_ptr },
                    0,
                );

                let cpu = crate::proc::mycpu();
                cpu.proc = None;
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

    // Snapshot a raw pointer to our saved-context slot, then RELEASE p.lock
    // before switching away. The context slot lives in the PROCS static (stable
    // for the life of the process), so the pointer stays valid after the guard
    // drops. Releasing the lock is essential: the scheduler re-locks every proc
    // on each pass, so if we held p.lock across the switch its frame would be
    // frozen with the lock held and the scheduler would deadlock re-locking us.
    let ctx_ptr = {
        let mut inner = p.lock();
        if inner.state == ProcState::Running {
            panic!("sched: running");
        }
        &mut inner.context as *mut Context
    };
    let cpu = crate::proc::mycpu();
    crate::arch::trap::context_switch(unsafe { &mut *ctx_ptr }, &cpu.context, 0);
}

pub fn scheduler_started() -> bool {
    unsafe { SCHEDULER_STARTED }
}