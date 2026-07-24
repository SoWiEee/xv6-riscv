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
    // Initialize process table
    for p in PROCS.iter() {
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
    // Free the user address space: uvmfree unmaps and frees the user pages in
    // [0, sz); dropping the page table then frees the page-table structure
    // (Drop clears the trampoline/trapframe leaves without freeing them).
    let sz = inner.sz;
    if let Some(mut pt) = inner.pagetable.take() {
        crate::mm::page_table::uvmfree(&mut pt, sz);
        drop(pt);
    }
    // The trapframe has its own page; Drop deliberately leaves it alone, so
    // release it here now that the process is gone.
    if !inner.trapframe.is_null() {
        let ppn = crate::mm::address::PhysPageNum::new((inner.trapframe as usize) >> 12);
        free_page(ppn);
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
    loop {
        // Interrupts stay ON while the scheduler idles looking for work; the
        // per-proc lock's push_off turns them off around each context switch.
        intr_on();
        for p in &PROCS {
            let mut inner = p.lock();
            if inner.state == ProcState::Runnable {
                inner.state = ProcState::Running;

                // Set current process for this CPU while holding the lock.
                let cpu = crate::proc::mycpu();
                cpu.proc = Some(p);

                // Do NOT touch satp/sstatus/sepc/stvec here. On a process's
                // first run new.ra = forkret, which releases p.lock and calls
                // usertrapret; that is what programs the return-to-user CSRs and
                // switches to the user page table (inside `userret`). The
                // scheduler runs entirely under the kernel page table.
                //
                // We keep p.lock HELD across the switch (xv6 discipline): this
                // guarantees the switch runs with interrupts off, and the far
                // side (forkret / sched's caller) releases the lock. `forget`
                // stops the guard's Drop from releasing it here.
                let ctx_ptr = &mut inner.context as *mut Context;
                core::mem::forget(inner);

                crate::arch::trap::context_switch(
                    &mut cpu.context,
                    unsafe { &*ctx_ptr },
                    0,
                );

                // Control returns here after the process switches back out (via
                // sched), which left p.lock held on our behalf; release it.
                let cpu = crate::proc::mycpu();
                cpu.proc = None;
                unsafe { p.lock.raw_release(); }
            }
        }
    }
}

pub fn yield_now() {
    let p = crate::proc::current_process();
    let mut inner = p.lock();
    inner.state = ProcState::Runnable;
    // Keep p.lock held across the switch (see `sched`); the scheduler releases
    // it, and re-acquires it before switching back into us, so we release it
    // here on return. `forget` prevents the guard Drop from releasing early.
    core::mem::forget(inner);
    sched();
    unsafe { p.lock.raw_release(); }
}

/// Switch from the current process back to the scheduler.
///
/// MUST be called while holding EXACTLY `p.lock` and nothing else, with the
/// process already moved out of the `Running` state (Runnable/Sleeping/Zombie).
/// The lock is intentionally kept held across the switch — this is what keeps
/// interrupts disabled during `swtch` — and the scheduler releases it on the
/// other side. We reach the saved-context slot through the held lock's data
/// pointer since the caller has `forget`-ten its guard.
pub fn sched() {
    let p = crate::proc::current_process();
    if !p.lock.holding() {
        panic!("sched: not holding p.lock");
    }
    if intr_get() {
        panic!("sched: interrupts enabled");
    }
    let inner = unsafe { &mut *p.lock.data_ptr() };
    if inner.state == ProcState::Running {
        panic!("sched: running");
    }
    let ctx_ptr = &mut inner.context as *mut Context;
    let cpu = crate::proc::mycpu();
    crate::arch::trap::context_switch(unsafe { &mut *ctx_ptr }, &cpu.context, 0);
}

pub fn scheduler_started() -> bool {
    unsafe { SCHEDULER_STARTED }
}