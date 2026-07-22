# Migration Guide: C xv6 to Rust xv6

This guide helps developers understand the differences between C xv6 and this Rust rewrite, and how to port code between them.

## Table of Contents

1. [Philosophy Differences](#philosophy-differences)
2. [Memory Management](#memory-management)
3. [Process Management](#process-management)
4. [File System](#file-system)
5. [Synchronization](#synchronization)
6. [Trap Handling](#trap-handling)
7. [System Calls](#system-calls)
8. [Common Patterns](#common-patterns)
9. [Porting Checklist](#porting-checklist)

---

## Philosophy Differences

| Aspect | C xv6 | Rust xv6 |
|--------|-------|----------|
| Memory safety | Manual, error-prone | Compile-time guaranteed |
| Ownership | Implicit conventions | Explicit via types |
| Concurrency | Locks + discipline | Locks + type system |
| Error handling | Return codes, global `errno` | `Result<T, E>` |
| Abstractions | Macros, void* | Generics, traits, RAII |

### Key Rust Concepts for C Developers

| C Pattern | Rust Equivalent |
|-----------|-----------------|
| `void*` + cast | Typed pointers, generics |
| Manual `free()` | `Drop` trait (RAII) |
| `struct` + separate init | `impl` with `new()` |
| Function pointers | Closures, `Fn` traits |
| Macros for code gen | Generics, macros |
| Global variables | `static` + `Mutex`/`SpinLock` |
| `errno` global | `Result<T, Errno>` return |

---

## Memory Management

### C xv6 Approach

```c
// Physical page allocator
struct run { struct run *next; };
struct { struct spinlock lock; struct run *freelist; } kmem;

void kinit() { ... }
void* kalloc() { ... }
void kfree(void* pa) { ... }

// Page table
pde_t* pgdir;
pde_t* walk(pde_t* pgdir, void* va, int alloc);
int mappages(pde_t* pgdir, void* va, uint size, uint pa, int perm);
```

### Rust xv6 Approach

```rust
// Typed addresses prevent mixing
pub struct PhysAddr(pub usize);
pub struct VirtAddr(pub usize);
pub struct PhysPageNum(pub usize);
pub struct VirtPageNum(pub usize);

// Frame allocator - encapsulated, thread-safe
pub fn alloc_page() -> Option<PhysPageNum>;
pub fn free_page(ppn: PhysPageNum);

// PageTable - RAII, owns mappings
pub struct PageTable { /* ... */ }
impl PageTable {
    pub fn new() -> Result<Self, KernelError>;
    pub fn map(&mut self, va: VirtAddr, pa: PhysAddr, flags: PteFlags) -> Result<()>;
    pub fn unmap(&mut self, va: VirtAddr) -> Result<()>;
    pub fn translate(&self, va: VirtAddr) -> Option<PhysAddr>;
}
```

### Migration Patterns

| C Code | Rust Code |
|--------|-----------|
| `kalloc()` returns `void*` | `alloc_page()` returns `Option<PhysPageNum>` |
| `kfree(ptr)` | `free_page(ppn)` - type-safe |
| `walk(pgdir, va, 1)` | `page_table.walk_mut(vpn)` |
| `mappages(pgdir, va, sz, pa, perm)` | `page_table.map(va, pa, flags)` |
| Manual `PTE` bit manipulation | `PteFlags` bitflags type |

### Address Translation

```c
// C: manual page table walk
pte_t* walk(pde_t* pgdir, void* va, int alloc) {
    for (int level = 2; level > 0; level--) {
        pde_t* pte = &pgdir[VPN(level, va)];
        if (!(*pte & PTE_V)) { ... }
        pgdir = (pde_t*)PTE2PA(*pte);
    }
    return &pgdir[VPN(0, va)];
}
```

```rust
// Rust: encapsulated in PageTable
impl PageTable {
    pub fn translate(&self, va: VirtAddr) -> Option<PhysAddr> {
        let mut table = self.root_ppn;
        for level in (0..3).rev() {
            let pte = self.read_pte(table, va.vpn(level))?;
            if !pte.is_valid() { return None; }
            if level == 0 { return Some(pte.pa() + va.page_offset()); }
            table = pte.ppn();
        }
        None
    }
}
```

---

## Process Management

### C xv6 Process

```c
enum procstate { UNUSED, USED, SLEEPING, RUNNABLE, RUNNING, ZOMBIE };

struct proc {
    struct spinlock lock;
    enum procstate state;
    void* chan;
    int killed;
    int xstate;
    int pid;
    struct proc* parent;
    struct context context;
    struct trapframe* trapframe;
    pagetable_t pagetable;
    uint sz;
    struct file* ofile[NOFILE];
    struct inode* cwd;
    char name[16];
    struct trapframe trapframe;  // embedded
};
```

### Rust xv6 Process

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcState {
    Unused, Used, Sleeping, Runnable, Running, Zombie,
}

pub struct Proc {
    inner: SpinLock<ProcInner>,
}

pub struct ProcInner {
    pub pid: usize,
    pub state: ProcState,
    pub pagetable: Option<PageTable>,  // Option for Unused state
    pub trapframe: *mut TrapFrame,     // Raw pointer for asm access
    pub context: Context,
    pub kstack: usize,
    pub name: [u8; 16],
    pub cwd: Option<Arc<Inode>>,
    pub ofile: [Option<Arc<File>>; NOFILE],
    pub killed: bool,
    pub xstate: i32,
    pub chan: usize,
    // No parent pointer - use process table lookup
}
```

### Key Differences

1. **Locking**: `inner: SpinLock<ProcInner>` - all mutable state protected
2. **No embedded trapframe**: Separate allocation, raw pointer for asm
3. **Option for optional fields**: `Option<PageTable>`, `Option<Arc<Inode>>`
4. **Arc for shared ownership**: `Arc<Inode>`, `Arc<File>` with reference counting
5. **No parent pointer**: Avoids cycles, lookup via PID when needed

### Process Creation

```c
// C
struct proc* allocproc() {
    struct proc* p;
    for (p = proc; p < &proc[NPROC]; p++) {
        acquire(&p->lock);
        if (p->state == UNUSED) { ... }
        release(&p->lock);
    }
    return 0;
}
```

```rust
// Rust
pub fn alloc_proc() -> Option<Arc<Proc>> {
    for p in PROCS.iter() {
        let mut inner = p.inner.acquire();
        if inner.state == ProcState::Unused {
            // Initialize...
            inner.state = ProcState::Used;
            return Some(Arc::clone(p));
        }
    }
    None
}
```

### Scheduler

```c
// C
void scheduler() {
    struct proc* p;
    for (;;) {
        for (p = proc; p < &proc[NPROC]; p++) {
            acquire(&p->lock);
            if (p->state == RUNNABLE) {
                p->state = RUNNING;
                c->proc = p;
                swtch(&c->context, &p->context);
                c->proc = 0;
            }
            release(&p->lock);
        }
    }
}
```

```rust
// Rust
pub fn sched() {
    loop {
        for p in PROCS.iter() {
            let mut inner = p.inner.acquire();
            if inner.state == ProcState::Runnable {
                inner.state = ProcState::Running;
                let ctx = &mut inner.context;
                // SAFETY: context switch in asm
                unsafe { context_switch(&mut mycpu().context, ctx); }
                // Back from switch
                inner.state = ProcState::Runnable; // or whatever it became
            }
        }
    }
}
```

---

## File System

### Buffer Cache

```c
// C
struct buf {
    int valid, disk;
    uint dev, blockno;
    struct spinlock lock;
    uint refcnt;
    struct buf* prev, *next;  // LRU list
    uchar data[BSIZE];
};
```

```rust
// Rust
pub struct Buf {
    dev: u32,
    blockno: u32,
    valid: bool,
    disk: bool,
    data: [u8; BSIZE],
    lock: Mutex<()>,  // or SpinLock
    refcnt: AtomicUsize,
    // LRU managed by BUF_CACHE
}

pub struct BUF_CACHE {
    bufs: Vec<Arc<Mutex<Buf>>>,
    // LRU logic
}
```

### Inodes

```c
// C
struct inode {
    uint dev, inum;
    int ref;
    struct spinlock lock;
    int valid;
    short type;
    short major, minor;
    short nlink;
    uint size;
    uint addrs[NDIRECT+1];
};
```

```rust
// Rust
pub struct Inode {
    dev: u32,
    inum: u32,
    inner: SpinLock<InodeInner>,
    weak_self: Weak<Inode>,  // for parent ref without cycle
}

pub struct InodeInner {
    pub valid: bool,
    pub type_: InodeType,
    pub major: u16,
    pub minor: u16,
    pub nlink: u32,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
}
```

### Logging

```c
// C
struct log {
    struct spinlock lock;
    int start;
    int size;
    int outstanding;
    int committing;
    int dev;
    struct buf* lhdr;
    struct buf* log[LOGBLOCKS];
};
```

```rust
// Rust
pub struct Log {
    lock: SpinLock<LogInner>,
    dev: u32,
    start: u32,
    size: u32,
}

pub struct LogInner {
    outstanding: usize,
    committing: bool,
    lhdr: Option<Arc<Mutex<Buf>>>,
    blocks: Vec<Option<Arc<Mutex<Buf>>>>,
}
```

---

## Synchronization

### Spinlocks

```c
// C
struct spinlock {
    uint locked;
    char* name;
    struct cpu* cpu;
    uint pcs[10];
};

void acquire(struct spinlock* lk) { ... }
void release(struct spinlock* lk) { ... }
```

```rust
// Rust
pub struct SpinLock<T> {
    locked: AtomicBool,
    name: &'static str,
    cpu: Option<usize>,
    // No pcs array - use backtrace crate if needed
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
    // Deref to T
}

impl<T> SpinLock<T> {
    pub fn acquire(&self) -> SpinLockGuard<'_, T> { ... }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) { self.lock.release(); }
}
```

### Sleep/Wakeup

```c
// C
void sleep(void* chan, struct spinlock* lk) {
    acquire(&wait_lock);
    release(lk);
    p->chan = chan;
    p->state = SLEEPING;
    sched();
    acquire(lk);
    release(&wait_lock);
}

void wakeup(void* chan) {
    acquire(&wait_lock);
    for (p = proc; p < &proc[NPROC]; p++) {
        if (p->state == SLEEPING && p->chan == chan) {
            p->state = RUNNABLE;
        }
    }
    release(&wait_lock);
}
```

```rust
// Rust
static WAIT_QUEUES: SpinLock<BTreeMap<usize, Vec<usize>>> = ...;

pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = current_process();
    let mut queues = WAIT_QUEUES.acquire();
    queues.entry(chan).or_default().push(p as *const Proc as usize);
    p.set_state(ProcState::Sleeping);
    p.set_chan(chan);
    
    // Release the lock (caller holds it, but no guard)
    unsafe { release_raw(&lock.locked); }
    sched();
    
    // Re-acquire
    lock.acquire();
    
    // Clean up wait queue
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        vec.retain(|&ptr| ptr != p as *const Proc as usize);
    }
}

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
```

---

## Trap Handling

### C xv6 Trap Frame

```c
struct trapframe {
    uint64 kernel_satp;
    uint64 kernel_sp;
    uint64 kernel_trap;
    uint64 epc;
    uint64 kernel_hartid;
    uint64 ra;
    uint64 sp;
    uint64 gp;
    uint64 tp;
    uint64 t0; ... uint64 s11;
};
```

### Rust xv6 Trap Frame

```rust
#[repr(C, align(16))]
pub struct TrapFrame {
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub kernel_trap: usize,
    pub epc: usize,
    pub sp: usize,
    pub ra: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    // ... all 32 registers
    // Generated via macro or include!
}
```

### Assembly Trampoline

Both use similar assembly for `uservec`/`userret`:
- Save user registers
- Switch to kernel page table (`satp`)
- Call Rust `usertrap()`
- Restore user registers
- Return to user mode

Key difference: Rust uses `extern "C"` functions called from asm.

```rust
// In trap.rs
#[no_mangle]
pub extern "C" fn usertrap() { ... }

#[no_mangle]
pub extern "C" fn kerneltrap() { ... }
```

---

## System Calls

### C xv6 Syscall

```c
// User
int fork() { return syscall(SYS_FORK); }

// Kernel
uint64 sys_fork() { ... }

void syscall() {
    int num = p->trapframe->a7;
    if (num >= 0 && num < NELEM(syscalls) && syscalls[num]) {
        p->trapframe->a0 = syscalls[num]();
    } else {
        p->trapframe->a0 = -1;
    }
}
```

### Rust xv6 Syscall

```rust
// User (user-lib)
pub fn fork() -> isize {
    syscall!(SYS_FORK) as isize
}

// Kernel
pub fn syscall(num: usize, args: [usize; 6]) -> isize {
    match num {
        SYS_FORK => sys_fork(),
        SYS_EXIT => sys_exit(args[0] as i32),
        // ...
        _ => -1,
    }
}

// In usertrap():
let num = tf.a7;
let args = [tf.a0, tf.a1, tf.a2, tf.a3, tf.a4, tf.a5];
tf.a0 = syscall(num, args) as usize;
```

### Key Differences

1. **Arguments**: Rust passes args explicitly as array, not via trapframe fields
2. **Return**: Rust returns `isize` (can be negative for errors)
3. **Types**: Rust uses proper types in syscall implementations

---

## Common Patterns

### C: Manual Resource Management

```c
struct inode* ip = namei(path);
if (ip == 0) return -1;
ilock(ip);
// ... use ip ...
iunlock(ip);
iput(ip);  // Must remember!
```

### Rust: RAII

```rust
let inode = namei(path)?;  // Returns Result<Arc<Inode>>
// Lock automatically acquired via guard
let guard = inode.lock();
// ... use guard (Deref to InodeInner) ...
drop(guard);  // Unlock automatically
// iput happens when Arc drops
```

### C: Linked Lists

```c
struct buf* b;
for (b = cache.head; b; b = b->next) { ... }
```

### Rust: Iterators

```rust
for buf in BUF_CACHE.iter() { ... }
// or
for buf in BUF_CACHE.bufs.iter() { ... }
```

### C: Function Pointers

```c
void (*devsw[])(struct buf*) = { 0, diskrw, ... };
devsw[dev](b);
```

### Rust: Traits/Enums

```rust
trait BlockDevice {
    fn read(&self, blockno: u32, buf: &mut [u8]);
    fn write(&self, blockno: u32, buf: &[u8]);
}

enum Device {
    Disk(Arc<dyn BlockDevice>),
    // ...
}
```

---

## Porting Checklist

When porting C xv6 code to Rust:

### Memory
- [ ] Replace `void*` with typed addresses (`PhysAddr`, `VirtAddr`, etc.)
- [ ] Use `Option<T>` instead of NULL pointers
- [ ] Replace manual `kalloc`/`kfree` with `alloc_page`/`free_page`
- [ ] Wrap page tables in `PageTable` with RAII
- [ ] Use `Arc<T>` for shared ownership (inodes, files)

### Processes
- [ ] Replace `struct proc` with `Proc` + `SpinLock<ProcInner>`
- [ ] Use `ProcState` enum instead of integer constants
- [ ] Remove parent pointer, use PID lookup
- [ ] Separate trapframe allocation

### File System
- [ ] Use `Arc<Mutex<Buf>>` for buffer cache
- [ ] Replace `struct inode` with `Inode` + `SpinLock<InodeInner>`
- [ ] Use `Option<Arc<Inode>>` for optional references
- [ ] Log transactions with `begin_op()`/`end_op()`

### Synchronization
- [ ] Replace `spinlock` with `SpinLock<T>` + guard pattern
- [ ] Use `SleepLock` for long-held locks
- [ ] Replace `sleep`/`wakeup` with wait queue API
- [ ] Use `push_off`/`pop_off` for interrupt nesting

### Traps
- [ ] Define `TrapFrame` with `#[repr(C, align(16))]`
- [ ] Write `usertrap`/`kerneltrap` as `extern "C" fn`
- [ ] Use assembly trampoline (same as C)

### System Calls
- [ ] Match syscall numbers exactly
- [ ] Return `isize` (negative = error)
- [ ] Use `UserBuffer` for copyin/copyout

### Error Handling
- [ ] Replace `errno` with `Result<T, Errno>`
- [ ] Use `?` operator for propagation
- [ ] Define `KernelError` enum for kernel errors

### Testing
- [ ] Add `#[cfg(test)]` unit tests
- [ ] Verify usertests pass
- [ ] Compare output with C xv6

---

## Useful Rust Crates

| Crate | Purpose |
|-------|---------|
| `riscv` | RISC-V register access, asm |
| `tock-registers` | Type-safe register manipulation |
| `linked-list-allocator` | Heap allocator |
| `spin` | SpinLock, Mutex, Once |
| `bitflags` | PTE flags, CPU status flags |
| `alloc` | `Vec`, `BTreeMap`, `Arc`, `Box` |
| `x86_64` | (for comparison) similar patterns |

---

## Resources

- [Rust Embedded Book](https://docs.rust-embedded.org/book/)
- [Rustonomicon](https://doc.rust-lang.org/nomicon/) - unsafe Rust
- [xv6 Book](https://pdos.csail.mit.edu/6.1810/) - C xv6 reference
- [This project's architecture.md](architecture.md) - Detailed architecture