// kernel/src/proc/syscall.rs
use crate::arch::trap::TrapFrame;
use crate::proc::current_process;
use crate::proc::scheduler::{alloc_proc, free_proc};
use crate::proc::process::Proc;
use crate::mm::page_table::{PageTable, uvmcreate, uvmalloc, uvmfree, uvmcopy, kernel_pagetable};
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::mm::address::{PhysAddr, PhysPageNum, VirtAddr};
use crate::arch::asm::{MAKE_SATP, r_satp, w_satp, sfence_vma};
use crate::sync::spinlock::SpinLock;
use crate::fs::{File, Inode, filealloc, fileclose, fileread, filewrite, filedup, iunlockput, iunlock, iupdate, namei, nameiparent, dirlink, dirlookup, ialloc, iput, begin_op, end_op, I_DIR, I_FILE, I_DEV};
use crate::arch::console::printk;
use crate::printk;
use core::fmt::Arguments;
use alloc::vec::Vec;
use alloc::string::String;

// System call numbers
pub const SYS_FORK: usize = 1;
pub const SYS_EXIT: usize = 2;
pub const SYS_WAIT: usize = 3;
pub const SYS_PIPE: usize = 4;
pub const SYS_READ: usize = 5;
pub const SYS_WRITE: usize = 6;
pub const SYS_CLOSE: usize = 7;
pub const SYS_KILL: usize = 8;
pub const SYS_EXEC: usize = 9;
pub const SYS_FSTAT: usize = 10;
pub const SYS_CHDIR: usize = 11;
pub const SYS_DUP: usize = 12;
pub const SYS_GETPID: usize = 13;
pub const SYS_SBRK: usize = 14;
pub const SYS_SLEEP: usize = 15;
pub const SYS_UPTIME: usize = 16;
pub const SYS_OPEN: usize = 17;
pub const SYS_MKNOD: usize = 18;
pub const SYS_UNLINK: usize = 19;
pub const SYS_LINK: usize = 20;
pub const SYS_MKDIR: usize = 21;
pub const SYS_MAX: usize = 21;

pub fn syscall() {
    let p = current_process();
    let tf = unsafe { &mut *p.lock().trapframe };
    let num = tf.a7;
    
    tf.a0 = match num {
        SYS_FORK => sys_fork() as usize,
        SYS_EXIT => { sys_exit(tf.a0 as i32); 0 },
        SYS_WAIT => sys_wait(tf.a0) as usize,
        SYS_PIPE => sys_pipe(tf.a0, tf.a1) as usize,
        SYS_READ => sys_read(tf.a0, tf.a1, tf.a2) as usize,
        SYS_WRITE => sys_write(tf.a0, tf.a1, tf.a2) as usize,
        SYS_CLOSE => sys_close(tf.a0) as usize,
        SYS_KILL => sys_kill(tf.a0) as usize,
        SYS_EXEC => sys_exec(tf.a0, tf.a1) as usize,
        SYS_FSTAT => sys_fstat(tf.a0, tf.a1) as usize,
        SYS_CHDIR => sys_chdir(tf.a0) as usize,
        SYS_DUP => sys_dup(tf.a0) as usize,
        SYS_GETPID => sys_getpid() as usize,
        SYS_SBRK => sys_sbrk(tf.a0) as usize,
        SYS_SLEEP => sys_sleep(tf.a0) as usize,
        SYS_UPTIME => sys_uptime() as usize,
        SYS_OPEN => sys_open(tf.a0, tf.a1, tf.a2) as usize,
        SYS_MKNOD => sys_mknod(tf.a0, tf.a1, tf.a2) as usize,
        SYS_UNLINK => sys_unlink(tf.a0) as usize,
        SYS_LINK => sys_link(tf.a0, tf.a1) as usize,
        SYS_MKDIR => sys_mkdir(tf.a0) as usize,
        _ => {
            crate::printk!("unknown syscall {}\n", num);
            -1isize as usize
        }
    };
}

fn sys_fork() -> isize {
    let p = current_process();
    let mut pinner = p.lock();
    
    // Allocate new process
    let np = match alloc_proc() {
        Some(proc) => proc,
        None => return -1,
    };
    let mut npinner = np.lock();
    
    // Copy page table
    npinner.pagetable = match uvmcreate() {
        Ok(pt) => Some(pt),
        Err(_) => {
            free_proc(np);
            return -1;
        }
    };
    
    if let (Some(src_pt), Some(dst_pt)) = (&pinner.pagetable, &mut npinner.pagetable) {
        if uvmcopy(src_pt, dst_pt, pinner.sz).is_err() {
            free_proc(np);
            return -1;
        }
    }
    
    npinner.sz = pinner.sz;
    npinner.parent = Some(p as *const crate::proc::process::Proc as *mut crate::proc::process::Proc);
    
    // Copy trapframe
    let tf = unsafe { &mut *npinner.trapframe };
    *tf = unsafe { *pinner.trapframe };
    tf.a0 = 0; // fork returns 0 in child
    
    // Copy file descriptors
    for i in 0..16 {
        if let Some(f) = &pinner.ofile[i] {
            npinner.ofile[i] = Some(filedup(f));
        }
    }
    
    // Copy cwd
    npinner.cwd = pinner.cwd.clone();
    
    // Copy name
    npinner.name = pinner.name;
    
    drop(npinner);
    drop(pinner);
    
    // Set child as runnable
    np.set_runnable();
    
    np.pid() as isize
}

pub fn sys_exit(code: i32) -> ! {
    let p = current_process();
    let mut inner = p.lock();
    inner.xstate = code;
    inner.state = ProcState::Zombie;
    
    // Close all open files
    for i in 0..16 {
        if let Some(f) = inner.ofile[i].take() {
            fileclose(f);
        }
    }
    
    // Release cwd
    inner.cwd = None;
    
    // Wake up parent
    if let Some(parent) = inner.parent {
        let parent = unsafe { &*parent };
        crate::proc::wakeup(parent as *const Proc as usize);
    }
    
    // Pass abandoned children to init
    // For now, just clean up
    drop(inner);
    
    // Schedule and never return
    crate::proc::sched();
    panic!("sys_exit: should not return");
}

fn sys_wait(addr: usize) -> isize {
    let p = current_process();
    loop {
        let mut found = false;
        let mut child_pid = 0;
        let mut child_xstate = 0;
        
        // Look for zombie children
        let mut inner = p.lock();
        for np in &crate::proc::scheduler::PROCS {
            let ninner = np.lock();
            if ninner.parent == Some(p as *const crate::proc::process::Proc as *mut crate::proc::process::Proc) {
                if ninner.state == ProcState::Zombie {
                    // Found zombie child
                    child_pid = ninner.pid;
                    child_xstate = ninner.xstate;
                    found = true;
                    
                    // Free the child
                    free_proc(np);
                    break;
                }
            }
        }
        drop(inner);
        
        if found {
            // Copy xstate to user address
            let p = current_process();
            let mut p_inner = p.lock();
            let pt = p_inner.pagetable.as_mut().unwrap();
            let va = crate::mm::address::VirtAddr(addr);
            if let Some(pa) = pt.translate(va) {
                let dst = pa.0 as *mut i32;
                unsafe { *dst = child_xstate; }
            }
            return child_pid as isize;
        }
        
        // No zombie child found, sleep
        if p.lock().killed {
            return -1;
        }
        
        let p = current_process();
        let wait_chan = p as *const _ as usize;
        let p_lock = p.lock();
        crate::proc::sleep(wait_chan, &p.lock);
    }
}

fn sys_pipe(fd0: usize, fd1: usize) -> isize {
    // TODO: implement pipe
    -1
}

fn sys_read(fd: usize, addr: usize, n: usize) -> isize {
    let p = current_process();
    let inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = inner.ofile[fd].as_ref().unwrap().clone();
    let pagetable = inner.pagetable.clone();
    drop(inner);
    
    // Translate user address
    let pt = pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(addr);
    if let Some(pa) = pt.translate(va) {
        let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, n) };
        let nread = fileread(f, dst);
        return nread as isize;
    }
    -1
}

fn sys_write(fd: usize, addr: usize, n: usize) -> isize {
    let p = current_process();
    let inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = inner.ofile[fd].as_ref().unwrap().clone();
    let pagetable = inner.pagetable.clone();
    drop(inner);
    
    // Translate user address
    let pt = pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(addr);
    if let Some(pa) = pt.translate(va) {
        let src = unsafe { core::slice::from_raw_parts(pa.0 as *const u8, n) };
        let nwritten = filewrite(f, src);
        return nwritten as isize;
    }
    -1
}

fn sys_close(fd: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = inner.ofile[fd].take().unwrap();
    drop(inner);
    
    fileclose(f);
    0
}

fn sys_kill(pid: usize) -> isize {
    for np in &crate::proc::scheduler::PROCS {
        let mut ninner = np.lock();
        if ninner.pid == pid {
            ninner.killed = true;
            if ninner.state == ProcState::Sleeping {
                ninner.state = ProcState::Runnable;
            }
            return 0;
        }
    }
    -1
}

fn sys_exec(path: usize, argv: usize) -> isize {
    // TODO: implement exec
    -1
}

fn sys_fstat(fd: usize, addr: usize) -> isize {
    // TODO: implement fstat
    -1
}

fn sys_chdir(path: usize) -> isize {
    // TODO: implement chdir
    -1
}

fn sys_dup(fd: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = filedup(inner.ofile[fd].as_ref().unwrap());
    
    // Find free fd
    for i in 0..16 {
        if inner.ofile[i].is_none() {
            inner.ofile[i] = Some(f);
            return i as isize;
        }
    }
    -1
}

fn sys_getpid() -> isize {
    current_process().pid() as isize
}

fn sys_sbrk(n: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    let old_sz = inner.sz;
    let new_sz = if (n as isize) >= 0 {
        old_sz + n
    } else {
        old_sz - (-(n as isize) as usize)
    };
    
    if let Some(pt) = &mut inner.pagetable {
        if (n as isize) >= 0 {
            if uvmalloc(pt, old_sz, new_sz).is_err() {
                return -1;
            }
        } else {
            uvmfree(pt, new_sz);
        }
    }
    
    inner.sz = new_sz;
    old_sz as isize
}

fn sys_sleep(ticks: usize) -> isize {
    let p = current_process();
    let wait_chan = ticks;
    crate::proc::sleep(wait_chan, &p.lock);
    0
}

fn sys_uptime() -> isize {
    crate::proc::ticks() as isize
}

fn sys_open(path: usize, flags: usize, mode: usize) -> isize {
    // TODO: implement open
    -1
}

fn sys_mknod(path: usize, major: usize, minor: usize) -> isize {
    // TODO: implement mknod
    -1
}

fn sys_unlink(path: usize) -> isize {
    // TODO: implement unlink
    -1
}

fn sys_link(old: usize, new: usize) -> isize {
    // TODO: implement link
    -1
}

fn sys_mkdir(path: usize) -> isize {
    // TODO: implement mkdir
    -1
}

use crate::proc::process::ProcState;