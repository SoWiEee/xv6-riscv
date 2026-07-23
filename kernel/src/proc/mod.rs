// kernel/src/proc/mod.rs
pub mod process;
pub mod scheduler;
pub mod syscall;
pub mod trapframe;

use crate::proc::process::{Proc, ProcState, NPROC, NOFILE, ProcInner};
use crate::arch::trap::Context;
use crate::arch::asm::{r_tp, make_satp};
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
    
    // Map trampoline
    pt.map(
        crate::mm::address::VirtAddr(crate::arch::asm::TRAMPOLINE),
        crate::mm::address::PhysAddr(crate::arch::asm::TRAMPOLINE),
        crate::arch::paging::PTE_R | crate::arch::paging::PTE_X
    ).unwrap();
    
    // Load init binary into user page table
    let entry = crate::elf::load_elf_from_bytes(crate::elf::INIT_BINARY, &mut pt)
        .expect("userinit: load_elf_from_bytes failed");
    crate::arch::console::printk(format_args!("userinit: entry={:#x}\n", entry));
    
    // Allocate user stack pages (4 pages = 16KB)
    let user_stack_top = 0x80000000; // Page-aligned top (2GB)
    let user_stack_bottom = user_stack_top - 4 * crate::arch::paging::PAGE_SIZE;
    crate::arch::console::printk(format_args!("userinit: mapping stack {:#x}..{:#x}\n", user_stack_bottom, user_stack_top));
    // Use pt.map directly since uvmalloc is for heap growth
    for vaddr in (user_stack_bottom..user_stack_top).step_by(crate::arch::paging::PAGE_SIZE) {
        let page = crate::mm::frame_allocator::kalloc().expect("userinit: failed to alloc stack page");
        pt.map(crate::mm::address::VirtAddr(vaddr), page.to_paddr(), crate::arch::paging::PTE_R | crate::arch::paging::PTE_W | crate::arch::paging::PTE_U).expect("userinit: failed to map stack page");
    }
    // Verify mapping
    let test_addr = user_stack_top - 0x20; // Near top
    if let Some(pa) = pt.translate(crate::mm::address::VirtAddr(test_addr)) {
        crate::arch::console::printk(format_args!("userinit: stack mapping verified at {:#x} -> {:#x}\n", test_addr, pa.0));
    } else {
        crate::arch::console::printk(format_args!("userinit: stack mapping FAILED at {:#x}\n", test_addr));
    }
    
    inner.pagetable = Some(pt);
    inner.sz = user_stack_top;
    
    // Set up trapframe
    let tf = unsafe { &mut *inner.trapframe };
    // kernel_satp should be the user page table's full satp value (mode + PPN) for userret to switch to
    if let Some(pt) = &inner.pagetable {
        tf.kernel_satp = crate::mm::address::PhysPageNum::new(make_satp(pt.root_ppn().0));
    } else {
        tf.kernel_satp = crate::mm::page_table::kernel_pagetable();
    }
    tf.kernel_sp = inner.kstack + 4096;
    tf.kernel_trap = crate::arch::trap::usertrap as usize;
    tf.epc = entry;
    crate::arch::console::printk(format_args!("userinit: tf.epc={:#x} tf@={:#x}\n", tf.epc, inner.trapframe as usize));
    
    // Set up user stack
    let (sp, argv_ptr) = crate::elf::setup_user_stack(
        inner.pagetable.as_mut().unwrap(),
        &[alloc::string::String::from("init")],
        user_stack_top
    ).expect("userinit: setup_user_stack failed");
    tf.sp = sp;
    tf.a0 = 1; // argc
    tf.a1 = argv_ptr; // argv pointer
    
    // Set up context for first return to user
    inner.context.ra = crate::arch::trap::userret as usize;
    inner.context.sp = inner.kstack + 4096;
    
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

/// Sleep on a channel, releasing the given lock.
/// The lock must be held before calling sleep.
/// This matches xv6's sleep(chan, lock) signature.
/// Note: The caller must hold the lock (have called acquire) but NOT hold a SpinLockGuard.
/// This function will release the lock, sleep, and re-acquire it.
pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = current_process();
    let mut queues = WAIT_QUEUES.acquire();
    queues.entry(chan).or_default().push(p as *const Proc as usize);
    p.set_state(ProcState::Sleeping);
    p.set_chan(chan);
    // Release the wait queue lock before releasing the external lock
    drop(queues);
    // Release the lock by manually unlocking
    // SAFETY: The caller guarantees the lock is held but no guard exists
    unsafe {
        crate::sync::spinlock::release_raw(&lock.locked);
    }
    sched();
    // Re-acquire the lock
    lock.acquire();
    // Remove from wait queue after wakeup
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        vec.retain(|&ptr| ptr != p as *const Proc as usize);
    }
}

/// Wake up all processes sleeping on a channel.
pub fn wakeup(chan: usize) {
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        let ptrs: Vec<usize> = vec.drain(..).collect();
        for p_ptr in ptrs {
            let p = unsafe { &*(p_ptr as *const Proc) };
            if p.state() == ProcState::Sleeping {
                p.set_state(ProcState::Runnable);
            }
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