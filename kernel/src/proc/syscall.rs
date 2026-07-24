// kernel/src/proc/syscall.rs
use crate::arch::trap::TrapFrame;
use crate::proc::current_process;
use crate::proc::scheduler::{alloc_proc, free_proc};
use crate::proc::process::Proc;
use crate::mm::page_table::{PageTable, uvmcreate, uvmalloc, uvmfree, uvmcopy, kernel_pagetable};
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::mm::address::{PhysAddr, PhysPageNum, VirtAddr};
use crate::arch::asm::{make_satp, r_satp, w_satp, sfence_vma, TRAMPOLINE};
use crate::sync::spinlock::SpinLock;
use crate::fs::{File, Inode, filealloc, fileclose, filewrite, filedup, fileread, iupdate, namei, nameiparent, dirlink, dirlookup, ialloc, iput, begin_op, end_op, I_DIR, I_FILE, I_DEV};
use crate::arch::console::printk;
use crate::printk;
use crate::elf::{load_elf, setup_user_stack};
use core::fmt::Arguments;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;

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

pub fn proc_syscall() {
    let p = current_process();
    
    // Get trapframe pointer without holding the lock across the syscall
    let tf_ptr = {
        let inner = p.lock();
        inner.trapframe
    };
    let tf = unsafe { &mut *tf_ptr };
    
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

    // Allocate the child FIRST, before locking the parent. `alloc_proc` iterates
    // over all procs and locks each one in turn; if we held the parent's lock
    // here it would try to re-lock the parent (the first entry in PROCS) and
    // self-deadlock on the spinlock. This mirrors xv6, where `fork` calls
    // `allocproc()` without holding `p->lock`.
    let np = match alloc_proc() {
        Some(proc) => proc,
        None => return -1,
    };
    let mut npinner = np.lock();

    // Now it is safe to lock the parent. Lock order is child-before-parent and
    // is used consistently in fork.
    let mut pinner = p.lock();

    // Copy page table
    npinner.pagetable = match uvmcreate() {
        Ok(pt) => Some(pt),
        Err(_) => {
            drop(npinner);
            free_proc(np);
            return -1;
        }
    };

    // uvmcreate only maps the trampoline. The child also needs ITS OWN trapframe
    // mapped at the fixed TRAPFRAME address, or the trampoline `uservec` will
    // fault (and fault-loop) on the child's first trap. Mirrors userinit.
    {
        let tf_pa = npinner.trapframe as usize;
        if let Some(pt) = npinner.pagetable.as_mut() {
            pt.map(
                crate::mm::address::VirtAddr(crate::arch::asm::TRAPFRAME),
                crate::mm::address::PhysAddr(tf_pa),
                crate::arch::paging::PTE_R | crate::arch::paging::PTE_W,
            )
            .unwrap();
        }
    }

    if let (Some(src_pt), Some(dst_pt)) = (&pinner.pagetable, &mut npinner.pagetable) {
        if uvmcopy(src_pt, dst_pt, pinner.sz).is_err() {
            drop(npinner);
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
            npinner.ofile[i] = Some(Arc::new(filedup(&f)));
        }
    }
    
    // Copy cwd, taking our own reference (xv6 idup) so the child's iput on
    // exit does not free an inode the parent still points at.
    npinner.cwd = pinner.cwd;
    if let Some(ptr) = pinner.cwd {
        crate::fs::idup(unsafe { &*ptr });
    }

    // Copy name
    npinner.name = pinner.name;

    // Set up the child's kernel context so the scheduler enters `forkret` on its
    // first run (which heads out to user mode via usertrapret), on the child's
    // own kernel stack. alloc_proc zeroed the context, so without this the
    // scheduler would `ret` to address 0.
    npinner.context = crate::arch::trap::Context::new();
    npinner.context.ra = crate::arch::trap::forkret as usize;
    npinner.context.sp = npinner.kstack + crate::arch::paging::PAGE_SIZE;

    drop(npinner);
    drop(pinner);
    
    // Set child as runnable
    np.set_runnable();
    
    np.pid() as isize
}

pub fn sys_exit(code: i32) -> ! {
    let p = current_process();

    // Close all open files. fileclose may block on a sleeplock, so we must NOT
    // hold p.lock across it: take each fd out under a brief lock, then close.
    for i in 0..16 {
        let f = p.lock().ofile[i].take();
        if let Some(f) = f {
            fileclose(&f);
        }
    }

    // Drop the cwd reference.
    let _cwd = p.lock().cwd.take();

    // Serialise with wait()/exit() via the global wait_lock, wake our parent,
    // then mark ourselves Zombie under p.lock. Release wait_lock but keep p.lock
    // held across the switch to the scheduler (xv6 discipline). Never returns.
    let wl = crate::proc::WAIT_LOCK.acquire();
    let parent = unsafe { (*p.lock.data_ptr()).parent };
    if let Some(parent) = parent {
        crate::proc::wakeup(parent as usize);
    }
    let mut inner = p.lock();
    inner.xstate = code;
    inner.state = ProcState::Zombie;
    drop(wl);
    core::mem::forget(inner);

    crate::proc::sched();
    panic!("sys_exit: should not return");
}

fn sys_wait(addr: usize) -> isize {
    let p = current_process();
    let p_ptr = p as *const crate::proc::process::Proc as *mut crate::proc::process::Proc;

    // Hold the global wait_lock across the whole loop, exactly like xv6. This
    // serialises us against a child's exit()+wakeup and prevents a lost wakeup.
    let mut wl = crate::proc::WAIT_LOCK.acquire();
    loop {
        let mut found = false;
        let mut have_kids = false;
        let mut child_pid = 0;
        let mut child_xstate = 0;

        // Scan the process table for our children. We never lock p itself here
        // (it is in PROCS): p is never its own child, and skipping it also avoids
        // a self-deadlock on the spinlock.
        for np in &crate::proc::scheduler::PROCS {
            if core::ptr::eq(np as *const _, p as *const _) {
                continue;
            }
            let ninner = np.lock();
            if ninner.parent == Some(p_ptr) {
                have_kids = true;
                if ninner.state == ProcState::Zombie {
                    child_pid = ninner.pid;
                    child_xstate = ninner.xstate;
                    found = true;
                    drop(ninner); // release before free_proc re-acquires np.lock
                    free_proc(np);
                    break;
                }
            }
        }

        if found {
            // Copy the child's exit status back to the parent's user address.
            let mut p_inner = p.lock();
            if let Some(pt) = p_inner.pagetable.as_mut() {
                let va = crate::mm::address::VirtAddr(addr);
                if let Some(pa) = pt.translate(va) {
                    unsafe { *(pa.0 as *mut i32) = child_xstate; }
                }
            }
            drop(p_inner);
            drop(wl);
            return child_pid as isize;
        }

        // No zombie yet. Give up if we have no children or were killed.
        if !have_kids || p.lock().killed {
            drop(wl);
            return -1;
        }

        // Sleep on our own proc pointer, atomically releasing wait_lock. `sleep`
        // expects a HELD lock (not p's own) and releases it via release_raw, so
        // `forget` prevents the guard Drop from double-releasing. On return
        // wait_lock is released, so re-acquire it for the next iteration.
        core::mem::forget(wl);
        crate::proc::sleep(p_ptr as usize, &crate::proc::WAIT_LOCK);
        wl = crate::proc::WAIT_LOCK.acquire();
    }
}

fn sys_pipe(fd0: usize, fd1: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Find two free file descriptors first
    let mut read_fd = None;
    let mut write_fd = None;
    for i in 0..16 {
        if inner.ofile[i].is_none() {
            if read_fd.is_none() {
                read_fd = Some(i);
            } else if write_fd.is_none() {
                write_fd = Some(i);
                break;
            }
        }
    }
    
    let (read_fd, write_fd) = match (read_fd, write_fd) {
        (Some(r), Some(w)) => (r, w),
        _ => return -1,
    };
    
    // Allocate pipe and create files
    let pipe = crate::fs::pipe::Pipe::new();
    let read_file = crate::fs::File::new_pipe(pipe.clone(), true, false);
    let write_file = crate::fs::File::new_pipe(pipe, false, true);
    
    inner.ofile[read_fd] = Some(Arc::new(read_file));
    inner.ofile[write_fd] = Some(Arc::new(write_file));
    
    drop(inner);
    
    // Copy file descriptors to user space
    let p = current_process();
    let mut p_inner = p.lock();
    let pt = p_inner.pagetable.as_mut().unwrap();
    let va0 = crate::mm::address::VirtAddr(fd0);
    let va1 = crate::mm::address::VirtAddr(fd1);
    
    if let (Some(pa0), Some(pa1)) = (pt.translate(va0), pt.translate(va1)) {
        let dst0 = pa0.0 as *mut i32;
        let dst1 = pa1.0 as *mut i32;
        unsafe {
            *dst0 = read_fd as i32;
            *dst1 = write_fd as i32;
        }
        0
    } else {
        -1
    }
}

fn sys_read(fd: usize, addr: usize, n: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = Arc::new(filedup(&inner.ofile[fd].as_ref().unwrap()));
    let pagetable = inner.pagetable.clone();
    drop(inner);
    
    // Translate user address
    let pt = pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(addr);
    if let Some(pa) = pt.translate(va) {
        let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, n) };
        let nread = fileread(&f, dst);
        return nread as isize;
    }
    -1
}

fn sys_write(fd: usize, addr: usize, n: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = Arc::new(filedup(&inner.ofile[fd].as_ref().unwrap()));
    let pagetable = inner.pagetable.clone();
    drop(inner);
    
    // Translate user address
    let pt = pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(addr);
    if let Some(pa) = pt.translate(va) {
        let src = unsafe { core::slice::from_raw_parts(pa.0 as *const u8, n) };
        let nwritten = filewrite(&f, src);
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
    
    fileclose(&f);
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
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1; // Path too long
            }
        }
    }
    
    // Translate argv array
    let va = crate::mm::address::VirtAddr(argv);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    let argv_ptr = pa.0 as *const usize;
    let mut args: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    unsafe {
        let mut i = 0;
        loop {
            let arg_ptr = *argv_ptr.add(i);
            if arg_ptr == 0 {
                break;
            }
            let va = crate::mm::address::VirtAddr(arg_ptr);
            if let Some(pa) = pt.translate(va) {
                let str_ptr = pa.0 as *const u8;
                let mut arg_str = alloc::string::String::new();
                let mut j = 0;
                loop {
                    let c = *str_ptr.add(j);
                    if c == 0 {
                        break;
                    }
                    arg_str.push(c as char);
                    j += 1;
                    if j > 256 {
                        return -1;
                    }
                }
                args.push(arg_str);
            } else {
                return -1;
            }
            i += 1;
            if i > 32 {
                return -1; // Too many arguments
            }
        }
    }
    
    drop(inner);

    // Try to open the file
    let inode = match crate::fs::namei(&path_str) {
        Ok(inode) => inode,
        Err(_) => return -1,
    };

    // Check if it's a regular file
    inode.lock();
    if inode.typ() != crate::fs::InodeType::File {
        inode.unlock();
        crate::fs::iput(inode);
        return -1;
    }
    inode.unlock();
    
    // Create a file to read the ELF
    let file = crate::fs::filealloc().ok_or(-1).unwrap();
    {
        let mut file_inner = file.inner();
        file_inner.typ = crate::fs::FileType::Inode;
        file_inner.readable = true;
        file_inner.writable = false;
        file_inner.inode = Some(inode);
        file_inner.off = 0;
    }
    
    // Create new page table for the process
    let mut new_pt = match crate::mm::page_table::uvmcreate() {
        Ok(pt) => pt,
        Err(_) => {
            crate::fs::fileclose(&file);
            return -1;
        }
    };

    // uvmcreate only maps the trampoline. Like fork/userinit, the fresh page
    // table must also map THIS process's trapframe at the fixed TRAPFRAME
    // address, or the trampoline `uservec` faults (and fault-loops) on the
    // first trap after exec returns to user mode.
    {
        let tf_pa = current_process().lock().trapframe as usize;
        if new_pt
            .map(
                crate::mm::address::VirtAddr(crate::arch::asm::TRAPFRAME),
                crate::mm::address::PhysAddr(tf_pa),
                crate::arch::paging::PTE_R | crate::arch::paging::PTE_W,
            )
            .is_err()
        {
            crate::fs::fileclose(&file);
            crate::mm::page_table::uvmfree(&mut new_pt, 0);
            return -1;
        }
    }

    // Load ELF executable
    let entry_point = match load_elf(&file, &mut new_pt) {
        Ok(entry) => entry,
        Err(e) => {
            crate::printk!("load_elf failed: {}\n", e);
            crate::fs::fileclose(&file);
            crate::mm::page_table::uvmfree(&mut new_pt, 0);
            return -1;
        }
    };
    
    // Allocate and map the user stack (4 pages) below a fixed top, mirroring
    // userinit. setup_user_stack only *fills* these pages (via translate); it
    // does not map them, so they must exist first or the fill fails.
    let user_stack_top = 0x80000000usize;
    let user_stack_bottom = user_stack_top - 4 * crate::arch::paging::PAGE_SIZE;
    for vaddr in (user_stack_bottom..user_stack_top).step_by(crate::arch::paging::PAGE_SIZE) {
        let page = match crate::mm::frame_allocator::kalloc() {
            Some(p) => p,
            None => {
                crate::fs::fileclose(&file);
                crate::mm::page_table::uvmfree(&mut new_pt, 0);
                return -1;
            }
        };
        if new_pt
            .map(
                crate::mm::address::VirtAddr(vaddr),
                page.to_paddr(),
                crate::arch::paging::PTE_R | crate::arch::paging::PTE_W | crate::arch::paging::PTE_U,
            )
            .is_err()
        {
            crate::fs::fileclose(&file);
            crate::mm::page_table::uvmfree(&mut new_pt, 0);
            return -1;
        }
    }

    // Set up user stack
    let (sp, argv_ptr) = match setup_user_stack(&mut new_pt, &args, user_stack_top) {
        Ok(res) => res,
        Err(e) => {
            crate::printk!("setup_user_stack failed: {}\n", e);
            crate::fs::fileclose(&file);
            crate::mm::page_table::uvmfree(&mut new_pt, 0);
            return -1;
        }
    };
    
    let p = current_process();
    let mut inner = p.lock();
    
    // Free old page table
    if let Some(mut old_pt) = inner.pagetable.take() {
        crate::mm::page_table::uvmfree(&mut old_pt, inner.sz);
    }
    
    inner.pagetable = Some(new_pt);
    // sz must be the TOP of the stack region, not the current stack pointer:
    // the user heap (sbrk/malloc) grows upward from sz, so anchoring it at the
    // stack top keeps the heap above the stack. Setting sz = sp (mid-stack)
    // made the first malloc hand back stack memory and clobber argv. Mirrors
    // userinit.
    inner.sz = user_stack_top;
    
    // Set up trapframe for user entry. a1 carries argv; a0 (argc) is delivered
    // as this syscall's RETURN VALUE, because the dispatcher writes the return
    // value into tf.a0 after we return — so setting tf.a0 here would just be
    // overwritten. This matches xv6, where exec returns argc for exactly this
    // reason.
    let tf = unsafe { &mut *inner.trapframe };
    tf.epc = entry_point;
    tf.sp = sp;
    tf.a1 = argv_ptr; // argv pointer

    crate::fs::fileclose(&file);

    args.len() as isize // argc -> lands in a0 via the dispatcher
}

fn sys_fstat(fd: usize, addr: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = Arc::new(crate::fs::filedup(&inner.ofile[fd].as_ref().unwrap()));
    let pagetable = inner.pagetable.clone();
    drop(inner);
    
    let pt = pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(addr);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    let result = crate::fs::filestat(&f, pa.0);
    result
}

fn sys_chdir(path: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Look up the directory
    let inode = match crate::fs::namei(&path_str) {
        Ok(inode) => inode,
        Err(_) => return -1,
    };
    
    // Verify it's a directory
    inode.lock();
    if inode.typ() != crate::fs::InodeType::Dir {
        inode.unlock();
        crate::fs::iput(inode);
        return -1;
    }
    inode.unlock();
    
    // Update cwd, releasing the reference to the previous directory.
    let p = current_process();
    let mut inner = p.lock();
    let old = inner.cwd.replace(inode as *const Inode);
    drop(inner);
    if let Some(old_ptr) = old {
        crate::fs::iput(unsafe { &*old_ptr });
    }
    0
}

fn sys_dup(fd: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    if fd >= 16 || inner.ofile[fd].is_none() {
        return -1;
    }
    
    let f = Arc::new(filedup(&inner.ofile[fd].as_ref().unwrap()));
    
    // Find free fd
    for i in 0..16 {
        if inner.ofile[i].is_none() {
            inner.ofile[i] = Some(f);
            return i as isize;
        }
    }
    // f goes out of scope here, Arc drops
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
    // Yield until `ticks` timer ticks have elapsed. (A wakeup(&ticks) mechanism
    // would be more efficient, but there is no tick wait channel yet, and
    // yielding avoids sleeping on p.lock — which `sleep` locks internally.)
    let start = crate::proc::ticks();
    while crate::proc::ticks().wrapping_sub(start) < ticks {
        if crate::proc::is_killed(current_process()) {
            return -1;
        }
        crate::proc::yield_now();
    }
    0
}

fn sys_uptime() -> isize {
    crate::proc::ticks() as isize
}

fn sys_open(path: usize, flags: usize, mode: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Parse flags
    let readable = (flags & 0x1) == 0; // O_RDONLY = 0, so readable if not write-only
    let writable = (flags & 0x2) != 0 || (flags & 0x4) != 0; // O_WRONLY or O_RDWR
    let create = (flags & 0x200) != 0; // O_CREATE
    
    // Try to look up the file
    let inode = match crate::fs::namei(&path_str) {
        Ok(inode) => inode,
        Err(_) => {
            // File doesn't exist - create if O_CREATE
            if create {
                // Get parent directory and name
                let (parent, name) = match crate::fs::nameiparent(&path_str) {
                    Ok(res) => res,
                    Err(_) => return -1,
                };
                
                parent.lock();
                // Allocate new inode
                let new_inode = match crate::fs::ialloc(crate::fs::ROOTDEV, crate::fs::InodeType::File) {
                    Some(inode) => inode,
                    None => {
                        parent.unlock();
                        crate::fs::iput(parent);
                        return -1;
                    }
                };
                
                // Link it in parent directory
                if crate::fs::dirlink(parent, name, new_inode.inum()).is_err() {
                    parent.unlock();
                    crate::fs::iput(parent);
                    crate::fs::iput(new_inode);
                    return -1;
                }
                parent.unlock();
                crate::fs::iput(parent);
                new_inode
            } else {
                return -1;
            }
        }
    };
    
    // Check file type. Directories may be opened read-only (for `ls`); any
    // write access to a directory is rejected, matching xv6.
    inode.lock();
    let typ = inode.typ();
    if typ == crate::fs::InodeType::Dir && writable {
        inode.unlock();
        crate::fs::iput(inode);
        return -1;
    }
    if typ != crate::fs::InodeType::File
        && typ != crate::fs::InodeType::Device
        && typ != crate::fs::InodeType::Dir
    {
        inode.unlock();
        crate::fs::iput(inode);
        return -1;
    }
    inode.unlock();

    // Allocate file structure
    let file = crate::fs::filealloc().ok_or(-1).unwrap();
    let mut file_inner = file.inner();
    // Regular files and directories are read through the inode layer; only
    // true device nodes route to a driver.
    file_inner.typ = if typ == crate::fs::InodeType::Device {
        crate::fs::FileType::Device
    } else {
        crate::fs::FileType::Inode
    };
    file_inner.readable = readable;
    file_inner.writable = writable;
    // For device files, record the major number so filewrite/fileread can
    // route to the right driver (console = major 1).
    if typ == crate::fs::InodeType::Device {
        file_inner.major = inode.major();
    }
    file_inner.inode = Some(inode);
    file_inner.off = 0;
    drop(file_inner);
    
    // Find free fd
    let p = current_process();
    let mut inner = p.lock();
    let file_arc = Arc::new(file);
    for i in 0..16 {
        if inner.ofile[i].is_none() {
            inner.ofile[i] = Some(file_arc);
            return i as isize;
        }
    }
    -1
}

fn sys_mknod(path: usize, major: usize, minor: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Get parent directory and name
    let (parent, name) = match crate::fs::nameiparent(&path_str) {
        Ok(res) => res,
        Err(_) => return -1,
    };
    
    parent.lock();
    
    // Check if already exists
    if crate::fs::dirlookup_locked(parent, name).is_some() {
        parent.unlock();
        crate::fs::iput(parent);
        return -1;
    }
    
    // Allocate new inode
    let new_inode = match crate::fs::ialloc(crate::fs::ROOTDEV, crate::fs::InodeType::Device) {
        Some(inode) => inode,
        None => {
            parent.unlock();
            crate::fs::iput(parent);
            return -1;
        }
    };
    
    new_inode.lock();
    new_inode.set_major(major as u16);
    new_inode.set_minor(minor as u16);
    new_inode.unlock();
    
    // Link it in parent directory
    let result = crate::fs::dirlink(parent, name, new_inode.inum());
    parent.unlock();
    crate::fs::iput(parent);
    crate::fs::iput(new_inode);
    
    if result.is_ok() { 0 } else { -1 }
}

fn sys_unlink(path: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Get parent directory and name
    let (parent, name) = match crate::fs::nameiparent(&path_str) {
        Ok(res) => res,
        Err(_) => return -1,
    };
    
    parent.lock();
    
    // Look up the inode
    let inode = match crate::fs::dirlookup_locked(parent, name) {
        Some(inode) => inode,
        None => {
            parent.unlock();
            crate::fs::iput(parent);
            return -1;
        }
    };
    
    inode.lock();
    
    // Cannot unlink directories (use rmdir instead)
    if inode.typ() == crate::fs::InodeType::Dir {
        inode.unlock();
        crate::fs::iput(inode);
        parent.unlock();
        crate::fs::iput(parent);
        return -1;
    }
    
    // Decrement link count
    inode.dec_nlink();
    inode.unlock();
    
    // Remove directory entry
    // This is a simplified version - in reality we'd need to zero out the dirent
    // For now, we just decrement nlink and let iput handle cleanup when it hits 0
    
    crate::fs::iput(inode);
    parent.unlock();
    crate::fs::iput(parent);
    
    0
}

fn sys_link(old: usize, new: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate old path
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(old);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    let old_ptr = pa.0 as *const u8;
    let mut old_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *old_ptr.add(i);
            if c == 0 {
                break;
            }
            old_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    // Translate new path
    let va = crate::mm::address::VirtAddr(new);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    let new_ptr = pa.0 as *const u8;
    let mut new_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *new_ptr.add(i);
            if c == 0 {
                break;
            }
            new_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Look up the old file
    let old_inode = match crate::fs::namei(&old_str) {
        Ok(inode) => inode,
        Err(_) => return -1,
    };
    
    old_inode.lock();
    
    // Cannot link directories
    if old_inode.typ() == crate::fs::InodeType::Dir {
        old_inode.unlock();
        crate::fs::iput(old_inode);
        return -1;
    }
    
    // Increment link count
    old_inode.inc_nlink();
    old_inode.unlock();
    
    // Get parent of new path
    let (parent, name) = match crate::fs::nameiparent(&new_str) {
        Ok(res) => res,
        Err(_) => {
            crate::fs::iput(old_inode);
            return -1;
        }
    };
    
    parent.lock();
    
    // Check if new name already exists
    if crate::fs::dirlookup_locked(parent, name).is_some() {
        parent.unlock();
        crate::fs::iput(parent);
        crate::fs::iput(old_inode);
        return -1;
    }
    
    // Link the new name to the same inode
    let result = crate::fs::dirlink(parent, name, old_inode.inum());
    
    parent.unlock();
    crate::fs::iput(parent);
    crate::fs::iput(old_inode);
    
    if result.is_ok() { 0 } else { -1 }
}

fn sys_mkdir(path: usize) -> isize {
    let p = current_process();
    let mut inner = p.lock();
    
    // Translate path from user space
    let pt = inner.pagetable.as_ref().unwrap();
    let va = crate::mm::address::VirtAddr(path);
    let pa = match pt.translate(va) {
        Some(pa) => pa,
        None => return -1,
    };
    
    // Read path string from user memory
    let path_ptr = pa.0 as *const u8;
    let mut path_str = alloc::string::String::new();
    unsafe {
        let mut i = 0;
        loop {
            let c = *path_ptr.add(i);
            if c == 0 {
                break;
            }
            path_str.push(c as char);
            i += 1;
            if i > 256 {
                return -1;
            }
        }
    }
    
    drop(inner);
    
    // Get parent directory and name
    let (parent, name) = match crate::fs::nameiparent(&path_str) {
        Ok(res) => res,
        Err(_) => return -1,
    };
    
    parent.lock();

    // Check if already exists
    if crate::fs::dirlookup_locked(parent, name).is_some() {
        parent.unlock();
        crate::fs::iput(parent);
        return -1;
    }

    // Allocate new inode for directory
    let new_inode = match crate::fs::ialloc(crate::fs::ROOTDEV, crate::fs::InodeType::Dir) {
        Some(inode) => inode,
        None => {
            parent.unlock();
            crate::fs::iput(parent);
            return -1;
        }
    };

    new_inode.lock();

    // Create "." entry (Dirent::new zeroes the name, so just copy the label).
    let mut de = crate::fs::inode::Dirent::new();
    de.inum = new_inode.inum() as u16;
    de.name[..1].copy_from_slice(b".");
    new_inode.write(&de.as_bytes()[..16], 0, 16);

    // Create ".." entry
    let mut de = crate::fs::inode::Dirent::new();
    de.inum = parent.inum() as u16;
    de.name[..2].copy_from_slice(b"..");
    new_inode.write(&de.as_bytes()[..16], 16, 16);

    // Update the new directory's on-disk inode.
    iupdate(new_inode);
    new_inode.unlock();

    // Link it in parent directory
    let result = crate::fs::dirlink(parent, name, new_inode.inum());

    parent.unlock();
    crate::fs::iput(parent);
    crate::fs::iput(new_inode);

    if result.is_ok() { 0 } else { -1 }
}

use crate::proc::process::ProcState;