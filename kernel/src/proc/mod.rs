// kernel/src/proc/mod.rs
pub mod process;
pub mod scheduler;
pub mod syscall;
pub mod trapframe;

use crate::proc::process::{Proc, ProcState, NPROC, NOFILE, ProcInner};
use crate::arch::trap::Context;
use crate::arch::asm::{r_tp, make_satp, TRAMPOLINE};
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::mm::page_table::PageTable;
use crate::mm::address::PhysPageNum;
use crate::fs::Inode;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use core::sync::atomic::AtomicUsize;

/// Number of pages in a user process's stack. The stack sits directly above the
/// program image (with a guard page below it), and the heap grows above it — see
/// `sys_exec`/`userinit`. xv6 uses a single page; we use a few more to give
/// Rust's larger stack frames headroom.
pub const USER_STACK_PAGES: usize = 4;

pub struct Cpu {
    pub proc: Option<&'static Proc>,
    pub context: Context,
    pub noff: usize,
    pub intena: bool,
}

impl Cpu {
    const fn new() -> Self { 
        Self { 
            proc: None, 
            context: Context::new(), 
            noff: 0, 
            intena: false 
        } 
    }
}

static mut CPU: [Cpu; 8] = [const { Cpu::new() }; 8];

pub fn mycpu() -> &'static mut Cpu {
    let hart = r_tp();
    unsafe { &mut CPU[hart] }
}

pub fn current_process() -> &'static Proc {
    current_process_opt().expect("no current process")
}

/// Return the process running on the current CPU, if any.
///
/// Interrupts MUST be disabled across the `mycpu()` read and the `proc` load:
/// `mycpu()` reads `tp` to index the per-CPU array, and if a timer preempts and
/// migrates this process to another hart between reading `tp` and reading
/// `cpu.proc`, we would read the OLD cpu's `proc` (which the scheduler cleared
/// to `None` on migration) — the "no current process" panic, and, via other
/// `mycpu()` writers, corrupted per-CPU state. Mirrors xv6 `myproc()`.
pub fn current_process_opt() -> Option<&'static Proc> {
    crate::sync::spinlock::push_off();
    let p = mycpu().proc;
    crate::sync::spinlock::pop_off();
    p
}

static mut SCHEDULER_STARTED: bool = false;

pub fn started() -> bool {
    unsafe { SCHEDULER_STARTED }
}

pub fn set_scheduler_started() {
    unsafe { SCHEDULER_STARTED = true; }
}

/// Global timer-tick counter. A single shared cell (was previously two separate
/// function-local `static mut TICKS`, so `ticks()` always read 0 and any
/// `sys_sleep` spun forever). `AtomicUsize` also makes it correct when every
/// hart's timer calls `tick()`.
static TICKS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn tick() {
    use core::sync::atomic::Ordering;
    let n = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if n % 100 == 0 {
        // Wake up processes sleeping on the tick channel.
        crate::proc::wakeup(n);
    }
}

pub fn ticks() -> usize {
    TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn userinit() {
    // Create first user process
    let p = crate::proc::scheduler::alloc_proc().expect("userinit: alloc_proc failed");
    // Record init as the reparent target for orphaned children (xv6 initproc).
    crate::proc::set_initproc(p);

    // Resolve the root inode for cwd BEFORE taking the proc lock. `namei` walks
    // the buffer cache, and a buffer sleeplock release inside it calls `wakeup`,
    // which must never run while a proc lock is held (at boot there is no current
    // process, so wakeup cannot skip us by identity and would try to re-lock this
    // very proc). "/" is absolute, so this needs no current process. The inode
    // cache was already initialised by `fsinit()` at boot.
    let cwd = crate::fs::namei("/").ok();

    let mut inner = p.lock();
    
    // Create user page table
    let mut pt = PageTable::new().expect("userinit: failed to create page table");
    
    // Map the trampoline (uservec/userret) at TRAMPOLINE. It lives in the
    // kernel .text, so map the physical page that actually contains it (NOT
    // the TRAMPOLINE virtual address).
    unsafe extern "C" { fn uservec(); }
    let trampoline_pa = (uservec as usize / crate::arch::paging::PAGE_SIZE) * crate::arch::paging::PAGE_SIZE;
    pt.map(
        crate::mm::address::VirtAddr(crate::arch::asm::TRAMPOLINE),
        crate::mm::address::PhysAddr(trampoline_pa),
        crate::arch::paging::PTE_R | crate::arch::paging::PTE_X
    ).unwrap();

    // Map this process's trapframe at the fixed TRAPFRAME address so the
    // trampoline can reach it under the user page table. Supervisor-only (no U).
    pt.map(
        crate::mm::address::VirtAddr(crate::arch::asm::TRAPFRAME),
        crate::mm::address::PhysAddr(inner.trapframe as usize),
        crate::arch::paging::PTE_R | crate::arch::paging::PTE_W
    ).unwrap();

    // Load init binary into user page table
    let (entry, prog_end) = crate::elf::load_elf_from_bytes(crate::elf::INIT_BINARY, &mut pt)
        .expect("userinit: load_elf_from_bytes failed");

    // xv6 address-space layout: one guard page then the user stack directly
    // above the program image, keeping `sz` small (see `sys_exec` for the
    // rationale and the fork/exec/exit cost this avoids). The guard page is left
    // unmapped so a stack overflow faults cleanly; the heap grows above the top.
    let ps = crate::arch::paging::PAGE_SIZE;
    let sz0 = (prog_end + ps - 1) & !(ps - 1);
    let stack_base = sz0 + ps; // one guard page below the stack
    let user_stack_top = stack_base + USER_STACK_PAGES * ps;
    for vaddr in (stack_base..user_stack_top).step_by(ps) {
        let page = crate::mm::frame_allocator::kalloc().expect("userinit: failed to alloc stack page");
        pt.map(crate::mm::address::VirtAddr(vaddr), page.to_paddr(), crate::arch::paging::PTE_R | crate::arch::paging::PTE_W | crate::arch::paging::PTE_U).expect("userinit: failed to map stack page");
    }

    inner.pagetable = Some(pt);
    inner.sz = user_stack_top;

    // Set up the trapframe user state. The kernel_* fields are filled in by
    // usertrapret on the way out to user mode, so we only set epc/sp/argc/argv.
    let tf = unsafe { &mut *inner.trapframe };
    tf.epc = entry;

    // Set up user stack
    let (sp, argv_ptr) = crate::elf::setup_user_stack(
        inner.pagetable.as_mut().unwrap(),
        &[alloc::string::String::from("init")],
        user_stack_top
    ).expect("userinit: setup_user_stack failed");
    tf.sp = sp;
    tf.a0 = 1; // argc
    tf.a1 = argv_ptr; // argv pointer

    // First run starts at forkret (kernel code), which calls usertrapret to
    // enter user mode via the trampoline.
    inner.context.ra = crate::arch::trap::forkret as usize;
    inner.context.sp = inner.kstack + crate::arch::paging::PAGE_SIZE;
    
    inner.pid = 1;
    inner.state = ProcState::Runnable;
    inner.name = *b"init\0\0\0\0\0\0\0\0\0\0\0\0";
    
    // Install the cwd resolved above (no FS work while the proc lock is held).
    if let Some(inode) = cwd {
        inner.cwd = Some(inode as *const Inode);
    }

    drop(inner);
    
    // Make runnable
    p.set_runnable();
}

// Wait queues for sleep/wakeup - using usize (process pointer as usize) to avoid Send issues
static WAIT_QUEUES: SpinLock<BTreeMap<usize, Vec<usize>>> =
    SpinLock::new(BTreeMap::new(), "wait_queues");

/// Global lock protecting the parent/child relationship used by wait/exit,
/// mirroring xv6's `wait_lock`. It must be a distinct lock from any individual
/// `proc.lock`: `sleep` internally locks the sleeping proc (via `set_state`) and
/// `sched` locks it again, so the lock handed to `sleep` must NOT be that proc's
/// own lock. Holding this across the child-scan and across `exit`'s wakeup also
/// closes the lost-wakeup race.
pub static WAIT_LOCK: SpinLock<()> = SpinLock::new((), "wait_lock");

/// The first user process (init). When a process exits, its still-living
/// children are handed to init so init's `wait()` loop reaps them; mirrors
/// xv6's global `initproc`. Set once in `userinit`.
static INITPROC: core::sync::atomic::AtomicPtr<Proc> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

pub fn set_initproc(p: &'static Proc) {
    INITPROC.store(
        p as *const Proc as *mut Proc,
        core::sync::atomic::Ordering::SeqCst,
    );
}

/// Give any children of `p` to init, so init's `wait()` loop reaps them once
/// they exit (xv6 `reparent`). MUST be called with `WAIT_LOCK` held. Without
/// this, a child whose parent exits first is never waited on and leaks its proc
/// slot as a permanent Zombie once it exits.
pub fn reparent(p: *const Proc) {
    let ip = INITPROC.load(core::sync::atomic::Ordering::SeqCst);
    if ip.is_null() {
        return;
    }
    let mut reparented = false;
    for pp in &crate::proc::scheduler::PROCS {
        // Never our own child; skip to avoid a needless self-lock.
        if core::ptr::eq(pp as *const Proc, p) {
            continue;
        }
        let mut inner = pp.lock();
        if inner.parent == Some(p as *mut Proc) {
            inner.parent = Some(ip);
            reparented = true;
        }
    }
    // Wake init once (it rescans all its children on wake). Done AFTER the scan
    // so no proc lock is held across wakeup, preserving that invariant.
    if reparented {
        wakeup(ip as usize);
    }
}

/// Sleep on a channel, releasing the given lock.
/// The lock must be held before calling sleep.
/// This matches xv6's sleep(chan, lock) signature.
/// Note: The caller must hold the lock (have called acquire) but NOT hold a SpinLockGuard.
/// This function will release the lock, sleep, and re-acquire it.
/// Atomically release `lock` and sleep on `chan` until a matching `wakeup`.
///
/// xv6 discipline: the caller holds `lock` (which MUST NOT be this process's own
/// `p.lock`, since we lock it internally). We take `p.lock`, release `lock`
/// while holding it, mark ourselves Sleeping, and `sched()` to the scheduler
/// with `p.lock` still held — this is what makes the release-and-sleep atomic
/// against a concurrent `wakeup` (which must take `p.lock` to change our state),
/// and keeps interrupts disabled across the switch. On return `p.lock` is
/// released and `lock` is left RELEASED; the caller re-acquires it if it loops.
pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = current_process();

    // Acquire p.lock, then release the caller's condition lock. The caller must
    // hold `lock` with no live guard (it either forgot the guard or holds it
    // raw) so that this is the single matching release.
    let mut inner = p.lock();
    unsafe { crate::sync::spinlock::release_raw(&lock.locked); }
    inner.chan = chan;
    inner.state = ProcState::Sleeping;

    // Keep p.lock held across the switch; the scheduler releases it and
    // re-acquires it before switching back into us.
    core::mem::forget(inner);
    sched();

    // Woken: clear the channel and release p.lock (held by the scheduler on our
    // behalf across the switch back in).
    unsafe {
        (*p.lock.data_ptr()).chan = 0;
        p.lock.raw_release();
    }
}

/// Wake every process sleeping on `chan`.
///
/// Scans the process table (xv6 style), locking each proc to flip `Sleeping` ->
/// `Runnable`. Taking each `p.lock` is what synchronises with `sleep`, which
/// sets `Sleeping` under the same lock.
///
/// The ONLY proc skipped is the one currently running on this CPU — exactly
/// xv6's `p != myproc()`. That process cannot be Sleeping (it is executing this
/// call), so re-locking it would be pointless; more importantly, the invariant
/// this relies on is that **no proc lock is ever held across `wakeup`**. We must
/// NOT skip based on `p.lock.holding()`: that read races with another hart's
/// `acquire` (the owner field is written after the lock is taken, so a lock held
/// by another hart can transiently read as owned by us), which would silently
/// drop a wakeup. Using the stable running-process identity is race-free.
pub fn wakeup(chan: usize) {
    let cur = current_process_opt().map(|p| p as *const Proc);
    for p in &crate::proc::scheduler::PROCS {
        if Some(p as *const Proc) == cur {
            continue;
        }
        let mut inner = p.lock();
        if inner.state == ProcState::Sleeping && inner.chan == chan {
            inner.state = ProcState::Runnable;
        }
    }
}

pub fn sched() {
    crate::proc::scheduler::sched();
}

pub fn yield_now() {
    crate::proc::scheduler::yield_now();
}

pub fn is_killed(p: &Proc) -> bool {
    p.is_killed()
}

pub fn set_killed(p: &Proc) {
    p.kill();
}

pub fn kexit(code: i32) -> ! {
    crate::proc::syscall::sys_exit(code)
}