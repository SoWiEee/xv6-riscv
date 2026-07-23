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
pub fn context_switch(old: &mut Context, new: &Context) {
    unsafe { swtch(old as *mut Context, new as *const Context) }
}

/// Prepare trap frame for return to user mode.
/// 
/// Sets up stvec, sstatus, and sepc for the userret trampoline.
pub fn prepare_return(tf: &mut TrapFrame) {
    intr_off();
    let trampoline_uservec = TRAMPOLINE + (uservec as usize - TRAMPOLINE);
    w_stvec(trampoline_uservec);
    tf.kernel_satp = PhysPageNum::new(r_satp() & ((1 << 44) - 1));
    tf.kernel_sp = tf.kernel_sp; // set by caller
    tf.kernel_trap = usertrap as usize;
    tf.kernel_hartid = r_tp();
    let mut sstatus = r_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    w_sstatus(sstatus);
    w_sepc(tf.epc);
}

/// User mode trap handler.
/// 
/// Called from `uservec` trampoline. Handles syscalls, interrupts, and page faults.
/// Returns the kernel `satp` value for the trampoline to load.
#[unsafe(no_mangle)]
pub extern "C" fn usertrap() -> usize {
    // Save user PC
    let sepc = r_sepc();
    let scause = r_scause();
    let stval = r_stval();
    
    let p = crate::proc::current_process();
    
    if (r_sstatus() & SSTATUS_SPP) != 0 {
        panic!("usertrap: not from user mode");
    }
    
    w_stvec(kernelvec as usize);
    
    {
        let mut inner = p.lock();
        unsafe { inner.trapframe.as_mut().unwrap().epc = sepc; }
    }
    
    match scause {
        8 => { // syscall
            if crate::proc::is_killed(p) {
                crate::proc::kexit(-1);
            }
            {
                let mut inner = p.lock();
                unsafe { inner.trapframe.as_mut().unwrap().epc += 4; }
            }
            intr_on();
            crate::syscall::syscall();
        }
        scause if scause & (1 << 63) != 0 => { // interrupt
            let dev = crate::arch::interrupt::devintr();
            if dev == 2 { crate::proc::yield_now(); }
        }
        13 | 15 => { // page fault
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
    
    {
        let mut inner = p.lock();
        unsafe { prepare_return(inner.trapframe.as_mut().unwrap()); }
    }
    
    let satp = {
        let inner = p.lock();
        inner.pagetable.as_ref().map(|pt| make_satp(pt.root_ppn().0)).unwrap_or(0)
    };
    satp
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