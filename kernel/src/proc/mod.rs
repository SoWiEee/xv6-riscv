// kernel/src/proc/mod.rs
use crate::arch::trap::TrapFrame;
use crate::arch::trap::Context;
use crate::arch::paging::PhysPageNum;
use core::fmt::Write;

pub struct Process {
    pub pid: usize,
    pub trapframe: TrapFrame,
    pub context: Context,
    pub pagetable: PhysPageNum,
    pub killed: bool,
}

static mut PROCESSES: [Option<Process>; 64] = [const { None }; 64];
static mut CURRENT_PROC: *mut Process = core::ptr::null_mut();
static mut NPROC: usize = 0;
static mut TICKS: usize = 0;

pub fn procinit() {
    unsafe {
        NPROC = 0;
        CURRENT_PROC = core::ptr::null_mut();
    }
}

pub fn current_process() -> &'static mut Process {
    unsafe {
        if CURRENT_PROC.is_null() {
            panic!("current_process: no current process");
        }
        &mut *CURRENT_PROC
    }
}

pub fn current_process_opt() -> Option<&'static mut Process> {
    unsafe {
        if CURRENT_PROC.is_null() {
            None
        } else {
            Some(&mut *CURRENT_PROC)
        }
    }
}

pub fn is_killed(p: &Process) -> bool {
    p.killed
}

pub fn set_killed(p: &mut Process) {
    p.killed = true;
}

pub fn kexit(code: i32) -> ! {
    panic!("kexit: {}", code);
}

pub fn yield_now() {
    // TODO: implement context switch
    loop {
        crate::arch::asm::wfi();
    }
}

pub fn tick() {
    unsafe {
        TICKS += 1;
    }
}

pub fn scheduler() -> ! {
    loop {
        crate::arch::asm::wfi();
    }
}

pub fn userinit() {
    // Create first user process
    crate::arch::console::printk(format_args!("userinit: creating first user process\n"));
}

pub fn started() -> bool {
    unsafe { NPROC > 0 }
}