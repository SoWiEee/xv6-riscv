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
    kernelvec as *const () as usize
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
    intr_off();

    // While in user space, traps go to uservec in the trampoline page.
    let uservec_va = TRAMPOLINE + (uservec as *const () as usize & 0xFFF);
    w_stvec(uservec_va);

    // Kernel state uservec restores on the next trap from this process.
    let tf = unsafe { &mut *tf_ptr };
    tf.kernel_satp = PhysPageNum::new(r_satp()); // full kernel satp value
    tf.kernel_sp = kstack + crate::proc::scheduler::KSTACK_SIZE;
    tf.kernel_trap = usertrap as *const () as usize;
    tf.kernel_hartid = r_tp();

    // Return to user (SPP=0) with interrupts enabled there (SPIE=1).
    let mut sstatus = r_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    w_sstatus(sstatus);
    w_sepc(epc);

    // Jump to userret in the trampoline (mapped in both page tables). It
    // switches to the user page table and returns to user mode.
    let userret_va = TRAMPOLINE + (userret as *const () as usize & 0xFFF);
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

        if (r_sstatus() & SSTATUS_SPP) != 0 {
            panic!("usertrap: not from user mode");
        }

        // While executing in the kernel, take traps via kernelvec.
        w_stvec(kernelvec as *const () as usize);

        let p = crate::proc::current_process();

        // Save the user program counter.
        {
            let inner = p.lock();
            unsafe { inner.trapframe.as_mut().unwrap().epc = sepc; }
        }

        match scause {
            8 => { // environment call from user mode (syscall)
                if crate::proc::is_killed(p) {
                    crate::proc::kexit(-1);
                }
                // sepc points at the ecall; return to the following instruction.
                {
                    let inner = p.lock();
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
            crate::arch::console::printk(format_args!("usertrap: unexpected scause {:#x} pid={} sepc={:#x} stval={:#x}\n", scause, pid, sepc, stval));
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
/// Dump the kernel stack around `sp` at the point of a fatal kerneltrap, tagging
/// each word so the SMP saved-ra corruption can be diagnosed from the serial log
/// alone. `garbage` is the faulting sepc (the value some `ret` jumped to).
fn forensic_stack_dump(sp: usize, garbage: usize) {
    use crate::arch::console::printk;
    unsafe extern "C" {
        fn etext();
    }
    let text_lo = crate::arch::asm::KERNBASE;
    let text_hi = etext as *const () as usize;

    // kerneltrap frame is 96 bytes; kernelvec pushed a 256-byte register block
    // below it. So the fault-time register file is at [sp+96, sp+352) and the
    // fault-time sp (what the interrupted code was running on) is sp+352.
    let kv = sp + 96;
    let fault_ra = unsafe { core::ptr::read_volatile((kv + 0) as *const usize) };
    let fault_sp = sp + 352;

    // Which proc slot owns this stack?
    let start_slot = match crate::proc::scheduler::kstack_locate(sp) {
        Some((slot, off)) => {
            printk(format_args!(
                "FORENSIC sp={:#x} slot={} off={:#x} fault_sp={:#x} fault_ra={:#x} garbage_sepc={:#x}\n",
                sp, slot, off, fault_sp, fault_ra, garbage
            ));
            Some(slot)
        }
        None => {
            printk(format_args!(
                "FORENSIC sp={:#x} NOT in any kstack (!) fault_sp={:#x} fault_ra={:#x} garbage={:#x}\n",
                sp, fault_sp, fault_ra, garbage
            ));
            None
        }
    };

    // Decode the fault-time register file kernelvec saved (offsets match
    // kernelvec's push order in asm.S).
    let rd = |off: usize| unsafe { core::ptr::read_volatile((kv + off) as *const usize) };
    let names: [(&str, usize); 12] = [
        ("s0", 56), ("s1", 64), ("s2", 136), ("s3", 144), ("s4", 152), ("s5", 160),
        ("s6", 168), ("s7", 176), ("s8", 184), ("s9", 192), ("s10", 200), ("s11", 208),
    ];
    printk(format_args!("FORENSIC regs: ra={:#x} tp={:#x} t0={:#x} a0={:#x}\n",
        fault_ra, rd(24), rd(32), rd(72)));
    for (nm, off) in names {
        printk(format_args!("  {}={:#x}\n", nm, rd(off)));
    }

    // Dump from the FAULT-TIME sp upward (skipping our own kerneltrap/kernelvec
    // frames), so we see the interrupted code's real call chain. Never read past
    // this slot's top (the next slot's guard page is unmapped). Tag each word:
    // CODE (a plausible return address), KSTK (points into a kernel stack), GARB
    // (equals the faulting sepc). The frame whose saved-ra slot is missing/GARB
    // is the corruption victim; the CODE entries name the surrounding callers.
    for i in 0..96usize {
        let a = fault_sp + i * 8;
        // Stop before leaving the starting slot (avoids the neighbour guard).
        match (start_slot, crate::proc::scheduler::kstack_locate(a)) {
            (Some(s), Some((sl, _))) if sl == s => {}
            (None, _) if a >= text_lo => {}
            _ => break,
        }
        let v = unsafe { core::ptr::read_volatile(a as *const usize) };
        let mut tag = "    ";
        if v == garbage && garbage != 0 {
            tag = "GARB";
        } else if v >= text_lo && v < text_hi {
            tag = "CODE";
        } else if crate::proc::scheduler::kstack_locate(v).is_some() {
            tag = "KSTK";
        }
        printk(format_args!("  [{:#x}] = {:#018x} {}\n", a, v, tag));
    }
    printk(format_args!("FORENSIC end\n"));
}

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
        let gsepc = r_sepc();
        crate::arch::console::printk(format_args!("kerneltrap: scause={:#x} sepc={:#x} stval={:#x}\n", scause, gsepc, r_stval()));
        // Forensic dump — only on the already-dying path, so it adds nothing to
        // normal execution (won't perturb the SMP timing race we're chasing).
        let cur_sp: usize;
        unsafe { core::arch::asm!("mv {}, sp", out(reg) cur_sp, options(nomem, nostack)); }
        forensic_stack_dump(cur_sp, gsepc);
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