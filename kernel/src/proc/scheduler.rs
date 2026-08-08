// kernel/src/proc/scheduler.rs
use crate::proc::process::{Proc, ProcState, NPROC, NOFILE};
use crate::arch::trap::{TrapFrame, Context};
use crate::arch::asm::{intr_on, intr_get};
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::sync::mutex::Mutex;

pub static PROCS: [Proc; NPROC] = [const { Proc::new() }; NPROC];
pub static mut SCHEDULER_STARTED: bool = false;

static NEXT_PID: Mutex<usize> = Mutex::new(1);

// --- Deterministic, guard-paged kernel stacks -----------------------------
//
// Kernel stacks live in a fixed static array (one slot per process index)
// instead of being handed out by the frame allocator. Two reasons:
//   1. Deterministic addresses — a given proc slot always has the same kstack
//      VA, so a hardware watchpoint / crash post-mortem can name the victim.
//   2. Guard page — each slot reserves one unmapped page *below* the usable
//      stack. A kernel stack overflow then faults precisely (store page fault,
//      sepc = the offending instruction) instead of silently smashing whatever
//      physical page happens to sit next to an alloc_page()'d stack.
//
// Layout of slot `i` (low -> high address):
//   [ guard page (unmapped) ][ KSTACK_PAGES usable stack pages ]
// The stack pointer starts at the top and grows down toward the guard.
pub const KSTACK_PAGES: usize = 1; // usable stack pages (matches C xv6)
pub const KSTACK_GUARD_PAGES: usize = 1;
const KSTACK_SLOT_PAGES: usize = KSTACK_PAGES + KSTACK_GUARD_PAGES;
pub const KSTACK_SIZE: usize = KSTACK_PAGES * crate::arch::paging::PAGE_SIZE;
const KSTACK_SLOT_SIZE: usize = KSTACK_SLOT_PAGES * crate::arch::paging::PAGE_SIZE;

#[repr(C, align(4096))]
struct KStacks([[u8; KSTACK_SLOT_SIZE]; NPROC]);
static mut KSTACKS: KStacks = KStacks([[0u8; KSTACK_SLOT_SIZE]; NPROC]);

/// Base (lowest) address of proc slot `i`'s kstack region, i.e. the guard page.
fn kstack_slot_base(i: usize) -> usize {
    (&raw const KSTACKS as usize) + i * KSTACK_SLOT_SIZE
}

/// Base of the *usable* kernel stack for proc slot `i` (just above the guard).
/// `context.sp` / `kernel_sp` = this + KSTACK_SIZE (the stack top).
fn kstack_base(i: usize) -> usize {
    kstack_slot_base(i) + KSTACK_GUARD_PAGES * crate::arch::paging::PAGE_SIZE
}

/// Address of the guard page for proc slot `i` (to be left unmapped).
pub fn kstack_guard_addr(i: usize) -> usize {
    kstack_slot_base(i)
}

/// If `addr` falls inside the static kstack array, return `(slot, offset)`.
/// Used by the kerneltrap forensic dump to name which proc's stack an address
/// belongs to. `offset < KSTACK_GUARD_PAGES*PAGE_SIZE` means it's in the guard.
pub fn kstack_locate(addr: usize) -> Option<(usize, usize)> {
    let base = &raw const KSTACKS as usize;
    let total = NPROC * KSTACK_SLOT_SIZE;
    if addr < base || addr >= base + total {
        return None;
    }
    let off = addr - base;
    Some((off / KSTACK_SLOT_SIZE, off % KSTACK_SLOT_SIZE))
}

fn next_pid() -> usize {
    let mut pid = NEXT_PID.lock();
    *pid += 1;
    *pid
}

pub fn procinit() {
    // Initialize process table
    for p in PROCS.iter() {
        let mut inner = p.lock();
        inner.pid = 0; // Will be set on alloc
    }
}

pub fn alloc_proc() -> Option<&'static Proc> {
    for (i, p) in PROCS.iter().enumerate() {
        let mut inner = p.lock();
        if inner.state == ProcState::Unused {
            inner.state = ProcState::Used;
            inner.pid = next_pid();
            // Fixed, guard-paged kstack slot for this proc index.
            inner.kstack = kstack_base(i);
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
    // kstack is a fixed static slot tied to the proc index — never freed to the
    // frame allocator. Just drop the reference; alloc_proc re-derives it.
    inner.kstack = 0;
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
    // Save/restore `intena` around the switch. `noff`/`intena` are per-CPU, but a
    // process may resume on a DIFFERENT hart than it left from, whose `intena`
    // belongs to that hart's scheduler (typically true). Without carrying our own
    // value across the switch, the pop_off that eventually releases p.lock would
    // restore the wrong interrupt-enable state, re-enabling interrupts in a
    // window the process assumes they are off (e.g. between yield returning and
    // the trap frame being restored in kerneltrap/kernelvec) — a timer there
    // re-enters on the same kstack and smashes a saved ra. Mirrors xv6 `sched()`.
    let saved_intena = cpu.intena;
    crate::arch::trap::context_switch(unsafe { &mut *ctx_ptr }, &cpu.context, 0);
    crate::proc::mycpu().intena = saved_intena;
}

pub fn scheduler_started() -> bool {
    unsafe { SCHEDULER_STARTED }
}