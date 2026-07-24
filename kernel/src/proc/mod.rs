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
    mycpu().proc.expect("no current process")
}

pub fn current_process_opt() -> Option<&'static Proc> {
    mycpu().proc
}

static mut SCHEDULER_STARTED: bool = false;

pub fn started() -> bool {
    unsafe { SCHEDULER_STARTED }
}

pub fn set_scheduler_started() {
    unsafe { SCHEDULER_STARTED = true; }
}

pub fn tick() {
    // Increment ticks, wakeup sleepers
    static mut TICKS: usize = 0;
    unsafe {
        TICKS += 1;
        if TICKS % 100 == 0 {
            // Wake up sleepers every 100 ticks
            crate::proc::wakeup(TICKS);
        }
    }
}

pub fn ticks() -> usize {
    static mut TICKS: usize = 0;
    unsafe { TICKS }
}

pub fn userinit() {
    // Create first user process
    let p = crate::proc::scheduler::alloc_proc().expect("userinit: alloc_proc failed");
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
    let entry = crate::elf::load_elf_from_bytes(crate::elf::INIT_BINARY, &mut pt)
        .expect("userinit: load_elf_from_bytes failed");

    // Allocate user stack pages (4 pages = 16KB)
    let user_stack_top = 0x80000000; // Page-aligned top (2GB)
    let user_stack_bottom = user_stack_top - 4 * crate::arch::paging::PAGE_SIZE;
    // Use pt.map directly since uvmalloc is for heap growth
    for vaddr in (user_stack_bottom..user_stack_top).step_by(crate::arch::paging::PAGE_SIZE) {
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
    
    // Set cwd
    crate::fs::iinit();
    if let Ok(inode) = crate::fs::namei("/") {
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
/// Scans the process table (xv6 style), locking each proc except the caller to
/// flip `Sleeping` -> `Runnable`. Taking each `p.lock` is what synchronises with
/// `sleep`, which sets `Sleeping` under the same lock.
pub fn wakeup(chan: usize) {
    for p in &crate::proc::scheduler::PROCS {
        // Skip any proc whose lock THIS cpu already holds. wakeup is reachable
        // from code that legitimately holds a proc lock (e.g. userinit holds
        // initproc's lock across namei, and a buffer sleeplock release inside
        // namei calls wakeup) — re-locking it here would self-deadlock. A proc
        // we hold locked cannot be stably sleeping on `chan` anyway.
        if p.lock.holding() {
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