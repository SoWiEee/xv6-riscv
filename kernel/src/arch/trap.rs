// kernel/src/arch/trap.rs
//! Trap handling for RISC-V.
//!
//! Defines trap frame and context structures, and implements the trap
//! entry points called from assembly trampolines.

use super::asm::*;
use crate::mm::address::PhysPageNum;

/// User trap frame layout.
/// 
/// Matches the assembly trampoline expectations. 16-byte aligned for
/// efficient memory access. Contains all user registers plus kernel
/// state needed for return to user mode.
#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    /// Kernel page table (satp value)
    pub kernel_satp: PhysPageNum,
    /// Kernel stack pointer
    pub kernel_sp: usize,
    /// Kernel trap handler entry point
    pub kernel_trap: usize,
    /// User program counter (sepc)
    pub epc: usize,
    /// Hart ID
    pub kernel_hartid: usize,
    // User registers
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
}

impl TrapFrame {
    /// Create a zeroed trap frame.
    pub const fn new() -> Self {
        Self {
            kernel_satp: PhysPageNum::new(0), kernel_sp: 0, kernel_trap: 0, epc: 0,
            kernel_hartid: 0, ra: 0, sp: 0, gp: 0, tp: 0,
            t0: 0, t1: 0, t2: 0, s0: 0, s1: 0,
            s2: 0, s3: 0, s4: 0, s5: 0, s6: 0, s7: 0,
            s8: 0, s9: 0, s10: 0, s11: 0,
            a0: 0, a1: 0, a2: 0, a3: 0, a4: 0, a5: 0, a6: 0, a7: 0,
            t3: 0, t4: 0, t5: 0, t6: 0,
        }
    }
}

/// Kernel context for context switching.
/// 
/// Only contains callee-saved registers needed to resume a kernel thread.
/// Caller-saved registers are not preserved across context switch.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Context {
    pub ra: usize,
    pub sp: usize,
    pub s0: usize,
    pub s1: usize,
    pub s2: usize,
    pub s3: usize,
    pub s4: usize,
    pub s5: usize,
    pub s6: usize,
    pub s7: usize,
    pub s8: usize,
    pub s9: usize,
    pub s10: usize,
    pub s11: usize,
}

impl Context {
    /// Create a zeroed context.
    pub const fn new() -> Self {
        Self { ra: 0, sp: 0, s0: 0, s1: 0, s2: 0, s3: 0,
               s4: 0, s5: 0, s6: 0, s7: 0, s8: 0, s9: 0,
               s10: 0, s11: 0 }
    }
}

// Assembly functions - declare as public extern
unsafe extern "C" {
    fn uservec();
    pub fn userret();
    fn kernelvec();
    fn swtch(old: *mut Context, new: *const Context);
}

/// Get the address of the kernelvec trap handler.
pub fn kernelvec_addr() -> usize {
    kernelvec as usize
}

/// Switch from one context to another.
/// 
/// Called from scheduler to switch between processes.
pub fn context_switch(old: &mut Context, new: &Context, _tf_ptr: usize) {
    // The trapframe pointer is intentionally unused here: the trampoline
    // `userret` obtains the trapframe from `sscratch` (set by the scheduler
    // before switching). `swtch` follows the C ABI (a0 = old, a1 = new): it
    // saves the callee-saved registers into `*old`, restores them from `*new`,
    // and `ret`s to `new.ra`. On the first switch into a process, `new.ra`
    // points at the trampoline `userret`, which enters user mode via `sret`;
    // on later switches it returns here so this function must NOT be noreturn.
    unsafe { swtch(old as *mut Context, new as *const Context) }
}

/// Prepare trap frame for return to user mode.
/// 
/// Return to user space (xv6 `usertrapret`).
///
/// Fills in the trapframe fields that `uservec` will need on the next trap,
/// programs `stvec`/`sstatus`/`sepc`, then jumps into the trampoline `userret`
/// with the user `satp`. Never returns: `userret` `sret`s to user mode.
pub fn usertrapret(p: &'static crate::proc::process::Proc) -> ! {
    // Gather everything we need under the lock, then release it. The lock does
    // push_off/pop_off, which can re-enable interrupts on drop, so we must not
    // hold it while stvec points at uservec (see below).
    let (tf_ptr, kstack, user_satp, epc) = {
        let inner = p.lock();
        let tf_ptr = inner.trapframe;
        let root = inner
            .pagetable
            .as_ref()
            .map(|pt| pt.root_ppn().0)
            .expect("usertrapret: process has no page table");
        let epc = unsafe { (*tf_ptr).epc };
        (tf_ptr, inner.kstack, make_satp(root), epc)
    };

    // From here until `sret`, interrupts MUST stay off: once stvec points at
    // uservec, any trap taken while still on the kernel page table would run
    // uservec and touch TRAPFRAME (only mapped in the user page table) -> fault
    // loop. sret re-enables interrupts in user mode via sstatus.SPIE.
    crate::arch::console::printk(format_args!("usertrapret: intr_off, stvec={:#x}\n", TRAMPOLINE + (uservec as usize & 0xFFF)));
    intr_off();

    // While in user space, traps go to uservec in the trampoline page.
    let uservec_va = TRAMPOLINE + (uservec as usize & 0xFFF);
    w_stvec(uservec_va);

    // Kernel state uservec restores on the next trap from this process.
    let tf = unsafe { &mut *tf_ptr };
    tf.kernel_satp = PhysPageNum::new(r_satp()); // full kernel satp value
    tf.kernel_sp = kstack + crate::arch::paging::PAGE_SIZE;
    tf.kernel_trap = usertrap as usize;
    tf.kernel_hartid = r_tp();

    // Return to user (SPP=0) with interrupts enabled there (SPIE=1).
    let mut sstatus = r_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    crate::arch::console::printk(format_args!("usertrapret: sstatus={:#x} sepc={:#x} user_satp={:#x}\n", sstatus, epc, user_satp));
    w_sstatus(sstatus);
    w_sepc(epc);

    // Jump to userret in the trampoline (mapped in both page tables). It
    // switches to the user page table and returns to user mode.
    let userret_va = TRAMPOLINE + (userret as usize & 0xFFF);
    crate::arch::console::printk(format_args!("usertrapret: jumping to userret={:#x}\n", userret_va));
    let userret_fn: extern "C" fn(usize) -> ! = unsafe { core::mem::transmute(userret_va) };
    userret_fn(user_satp)
}

/// Entry point for a freshly scheduled process (its `Context::ra` points here).
///
/// Reached via `swtch` from the scheduler. This scheduler releases the process
/// lock before switching, so—unlike xv6—there is nothing to unlock here; we
/// just head out to user space.
#[unsafe(no_mangle)]
pub extern "C" fn forkret() -> ! {
    let p = crate::proc::current_process();
    // The scheduler switched into us while holding p.lock (xv6 discipline). We
    // are the far side of that switch, so we must release it before running.
    unsafe { p.lock.raw_release(); }
    usertrapret(p)
}

/// User mode trap handler.
    /// 
    /// Called from `uservec` trampoline. Handles syscalls, interrupts, and page faults.
    /// Returns the kernel `satp` value for the trampoline to load.
    #[unsafe(no_mangle)]
    pub extern "C" fn usertrap() -> ! {
        let sepc = r_sepc();
        let scause = r_scause();
        let stval = r_stval();

        crate::arch::console::printk(format_args!("usertrap: scause={:#x} sepc={:#x}\n", scause, sepc));

        if (r_sstatus() & SSTATUS_SPP) != 0 {
            panic!("usertrap: not from user mode");
        }

        // While executing in the kernel, take traps via kernelvec.
        w_stvec(kernelvec as usize);

        let p = crate::proc::current_process();

        // Save the user program counter.
        {
            let mut inner = p.lock();
            unsafe { inner.trapframe.as_mut().unwrap().epc = sepc; }
        }

        match scause {
            8 => { // environment call from user mode (syscall)
                if crate::proc::is_killed(p) {
                    crate::proc::kexit(-1);
                }
                // sepc points at the ecall; return to the following instruction.
                {
                    let mut inner = p.lock();
                    unsafe { inner.trapframe.as_mut().unwrap().epc += 4; }
                }
                intr_on();
                crate::syscall::syscall();
            }
        scause if scause & (1 << 63) != 0 => { // device / timer interrupt
            let dev = crate::arch::interrupt::devintr();
            if dev == 2 { crate::proc::yield_now(); }
        }
        13 | 15 => { // load / store page fault
            let read = scause == 13;
            let pt = {
                let inner = p.lock();
                inner.pagetable.as_ref().map(|pt| pt.root_ppn())
            };
            if let Some(root_ppn) = pt {
                if crate::mm::page_fault::handle_page_fault(
                    &mut crate::mm::page_table::PageTable::from_root(root_ppn),
                    stval,
                    read
                ).is_err() {
                    crate::proc::set_killed(p);
                }
            }
        }
        _ => {
            let pid = p.pid();
            crate::arch::console::printk(format_args!("usertrap: unexpected scause {:#x} pid={}\n", scause, pid));
            crate::proc::set_killed(p);
        }
    }

    if crate::proc::is_killed(p) {
        crate::proc::kexit(-1);
    }

    // Return to user space (never returns here).
    usertrapret(p)
}

/// Kernel mode trap handler.
/// 
/// Called from `kernelvec` trampoline. Handles timer interrupts and device interrupts.
/// Panics on unexpected traps.
#[unsafe(no_mangle)]
pub extern "C" fn kerneltrap() {
    let sepc = r_sepc();
    let sstatus = r_sstatus();
    let scause = r_scause();
    
    if (sstatus & SSTATUS_SPP) == 0 {
        panic!("kerneltrap: not from supervisor mode");
    }
    if intr_get() {
        panic!("kerneltrap: interrupts enabled");
    }
    
    let dev = crate::arch::interrupt::devintr();
    if dev == 0 {
        crate::arch::console::printk(format_args!("kerneltrap: scause={:#x} sepc={:#x} stval={:#x}\n", scause, r_sepc(), r_stval()));
        panic!("kerneltrap");
    }
    
    if dev == 2 {
        if let Some(_p) = crate::proc::current_process_opt() {
            crate::proc::yield_now();
        }
    }
    
    w_sepc(sepc);
    w_sstatus(sstatus);
}