# xv6-riscv Rust Architecture

This document provides a comprehensive overview of the xv6-riscv Rust kernel architecture.

## Table of Contents

1. [High-Level Structure](#high-level-structure)
2. [Memory Management](#memory-management)
3. [Process Management](#process-management)
4. [File System](#file-system)
5. [Synchronization](#synchronization)
6. [Trap Handling](#trap-handling)
7. [Device Drivers](#device-drivers)
8. [System Calls](#system-calls)
9. [User Space](#user-space)
10. [Build System](#build-system)

---

## High-Level Structure

```
xv6-rust/
├── kernel/                 # Kernel crate (no_std)
│   ├── src/
│   │   ├── arch/           # RISC-V architecture specifics
│   │   ├── mm/             # Memory management
│   │   ├── fs/             # File system
│   │   ├── proc/           # Process management
│   │   ├── sync/           # Synchronization primitives
│   │   ├── drivers/        # Device drivers
│   │   ├── syscall/        # System call handling
│   │   ├── trap/           # Trap handling
│   │   └── lib.rs          # Crate root
│   ├── Cargo.toml
│   └── memory.x            # Linker script for memory layout
├── user/                   # User-space programs (std)
├── user-lib/               # Shared user library
├── xtask/                  # Build automation
└── Cargo.toml              # Workspace root
```

### Key Design Principles

1. **Memory Safety Without Compromise**: All unsafe code is isolated in `arch/`, `mm/frame_allocator`, and `sync/`. Safe abstractions use Rust's type system to prevent bugs.

2. **Typed Addresses**: `PhysAddr`, `VirtAddr`, `PhysPageNum`, `VirtPageNum` newtypes prevent accidental mixing of address spaces.

3. **RAII Resource Management**: `PageTable` owns mappings and frees pages on drop. `SpinLockGuard` releases locks automatically.

4. **Capability-Based Access**: `UserBuffer` for `copyin`/`copyout` with bounds checking.

---

## Memory Management

### Physical Memory Layout (`memory.x`)

```
MEMORY
{
  KERNEL : ORIGIN = 0x80000000, LENGTH = 128M
  USER   : ORIGIN = 0x00000000, LENGTH = 2G   (virtual, per-process)
}
```

- Kernel loaded at `0x80000000` (physical and virtual identity-mapped)
- User space: 2GB virtual address space per process
- Trampoline page at `0xFFFFFFFFFFFFF000` (highest virtual page)

### Page Table (Sv39)

- **Page size**: 4096 bytes
- **Levels**: 3 (L2, L1, L0), 512 entries each
- **PTE format** (64-bit):
  - Bit 0: V (Valid)
  - Bit 1: R (Read)
  - Bit 2: W (Write)
  - Bit 3: X (Execute)
  - Bit 4: U (User)
  - Bit 5: G (Global)
  - Bit 6: A (Accessed)
  - Bit 7: D (Dirty)
  - Bits 10-53: PPN (Physical Page Number)

### Key Modules

#### `mm/address.rs`
Type-safe address wrappers:
```rust
pub struct PhysAddr(pub usize);
pub struct VirtAddr(pub usize);
pub struct PhysPageNum(pub usize);
pub struct VirtPageNum(pub usize);
```

#### `mm/frame_allocator.rs`
Physical page allocator (bitmap-based):
- `alloc_page() -> Option<PhysPageNum>`
- `free_page(ppn: PhysPageNum)`
- `kinit(start: PhysAddr, end: PhysAddr)` - initialize from linker script

#### `mm/page_table.rs`
Page table management:
- `PageTable::new()` - create empty page table
- `map(va: VirtAddr, pa: PhysAddr, flags: PteFlags)` - map page
- `unmap(va: VirtAddr)` - unmap page
- `translate(va: VirtAddr) -> Option<PhysAddr>` - walk page table
- `kernel_pagetable()` - global kernel page table

#### `mm/heap.rs`
Kernel heap allocator using `linked_list_allocator`:
- Global allocator for `alloc` crate
- Initialized after frame allocator

---

## Process Management

### Process Structure (`proc/process.rs`)

```rust
pub enum ProcState {
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

pub struct Proc {
    inner: SpinLock<ProcInner>,
    // ...
}

pub struct ProcInner {
    pub pid: usize,
    pub state: ProcState,
    pub pagetable: Option<PageTable>,
    pub trapframe: *mut TrapFrame,
    pub context: Context,
    pub kstack: usize,
    pub name: [u8; 16],
    pub cwd: Option<Arc<Inode>>,
    pub ofile: [Option<Arc<File>>; NOFILE],
    pub killed: bool,
    pub xstate: i32,
    pub chan: usize,        // wait channel
    // ...
}
```

### Scheduler (`proc/scheduler.rs`)

Simple round-robin scheduler:
- `alloc_proc() -> Option<Arc<Proc>>` - allocate new process
- `sched()` - context switch to scheduler
- `yield_now()` - yield current process
- `wakeup(chan)` / `sleep(chan, lock)` - wait queues

### Process Lifecycle

```
Unused -> Used -> Runnable -> Running -> Sleeping -> Runnable -> ... -> Zombie -> Unused
```

- `userinit()`: Creates first `init` process (PID 1)
- `fork()`: Copies parent's page table (COW not implemented yet)
- `exec()`: Loads ELF binary, replaces page table
- `exit()`: Sets state to Zombie, wakes parent
- `wait()`: Reaps zombie child

### Trap Frame (`proc/trapframe.rs`)

```rust
#[repr(C, align(16))]
pub struct TrapFrame {
    pub kernel_satp: usize,   // kernel page table
    pub kernel_sp: usize,     // kernel stack pointer
    pub kernel_trap: usize,   // usertrap function
    pub epc: usize,           // user PC
    pub sp: usize,            // user SP
    pub ra: usize,            // user RA
    pub gp: usize,            // user GP
    pub tp: usize,            // user TP
    pub t0: usize,            // registers...
    // ... all 32 general-purpose registers
}
```

---

## File System

### On-Disk Layout (same as C xv6)

```
Block 0:    Boot block
Block 1:    Superblock
Blocks 2-31: Log
Blocks 32-: Inodes
            Data blocks
```

### Key Structures

#### `fs/buf.rs` - Buffer Cache
- `Buf` - cached disk block with mutex
- `BUF_CACHE` - global LRU cache of `Arc<Mutex<Buf>>`
- `bread(dev, blockno) -> BufGuard` - read block
- `bwrite(guard)` - write block
- `bpin/bunpin` - reference counting for log

#### `fs/inode.rs` - Inode Layer
- `Inode` - in-memory inode with `Arc` reference counting
- `DiskInode` - on-disk inode structure
- `namei(path) -> Result<Arc<Inode>>` - path lookup
- `ialloc(dev, type) -> u32` - allocate inode
- `iget(dev, inum) -> Arc<Inode>` - get inode (with refcount)
- `iput(inode)` - release reference

#### `fs/log.rs` - Write-Ahead Logging
- Transaction API: `begin_op()`, `end_op()`
- `recover_from_log()` - replay on boot
- Log blocks store modified blocks before commit

#### `fs/file.rs` - File Descriptor Layer
- `File` - open file description (refcounted)
- `filealloc() -> Option<Arc<File>>`
- `fileread/filestat/filewrite` - operations

#### `fs/pipe.rs` - Pipes
- `Pipe` - pair of `File` (read/write ends)
- `pipealloc() -> (Arc<File>, Arc<File>)`

---

## Synchronization

### Primitives (`sync/`)

#### `spinlock.rs` - SpinLock
```rust
pub struct SpinLock<T> {
    locked: AtomicBool,
    name: &'static str,
    cpu: Option<usize>,  // for debugging
}

pub struct SpinLockGuard<'a, T> { /* implements Deref, DerefMut */ }
```
- Disables interrupts on acquire (IRQ-safe)
- Guard pattern ensures automatic release

#### `sleeplock.rs` - SleepLock
```rust
pub struct SleepLock<T> {
    locked: AtomicBool,
    condvar: Condvar,
}
```
- For long-held locks (e.g., inode locks)
- Uses `Condvar` for sleeping

#### `condvar.rs` - Condition Variables
```rust
pub struct Condvar {
    wait_queue: SpinLock<Vec<usize>>,  // process pointers
}
```
- `sleep(lock)` - atomically release lock and sleep
- `wakeup()` - wake one/all waiters

#### `mutex.rs` - Mutex (std-like)
- For non-interrupt contexts
- Simpler than SpinLock, no IRQ disable

### Interrupt Management

```rust
pub fn push_off() { /* disable interrupts, increment noff */ }
pub fn pop_off()  { /* decrement noff, enable if 0 */ }
pub fn intr_on()  { /* enable interrupts */ }
pub fn intr_off() { /* disable interrupts */ }
pub fn holding(lock: &SpinLock<_>) -> bool { /* check if current CPU holds lock */ }
```

---

## Trap Handling

### Architecture (`arch/trap.rs`)

#### Entry Points (Assembly in `arch/asm.rs`)
- `uservec` - user -> kernel (trampoline)
- `kernelvec` - kernel -> kernel
- `userret` - kernel -> user
- `context_switch` - process context switch

#### Trap Handling Flow

```
User Mode                    Kernel Mode
    │                             │
    ├─ ecall / interrupt ──────► │
    │                       uservec (asm)
    │                       │
    │                       ▼
    │                  save user regs
    │                       │
    │                       ▼
    │                  usertrap() (Rust)
    │                  ├─ syscall
    │                  ├─ page fault
    │                  ├─ timer interrupt
    │                  └─ external interrupt
    │                       │
    │                       ▼
    │                  userret (asm)
    │                       │
    │                       ▼
    ├─ restore user regs ◄───┤
    │                             │
```

#### Key Functions

- `usertrap()` - handle traps from user mode
- `kerneltrap()` - handle traps from kernel mode
- `userret()` - return to user mode
- `trapframe` manipulation

### Timer

- Uses `stimecmp` CSR and `ssip` SIP bit
- `tick()` called from timer interrupt
- Increments global `TICKS`, wakes sleepers every 100 ticks

---

## Device Drivers

### UART (`drivers/uart.rs`)
- 16550-compatible UART at `0x10000000`
- `uart_init()` - initialize
- `uart_putc(c)` - output character
- `uart_getc() -> Option<u8>` - input character
- `uart_intr()` - interrupt handler

### Virtio Block (`drivers/virtio.rs`)
- Virtio block device at `0x10001000`
- `virtio_init()` - initialize, negotiate features
- `virtio_disk_rw(buf, write)` - read/write block
- `virtio_intr()` - interrupt handler

### Console (`drivers/console.rs`)
- `console_init()` - initialize console
- `printk(fmt)` / `printk_fmt(args)` - kernel printf
- `console_intr()` - input interrupt

### PLIC (`arch/interrupt.rs`)
- Platform-Level Interrupt Controller
- `plic_init()` / `plic_inithart()` - initialize
- `plic_enable(irq, hart)` - enable interrupt

---

## System Calls

### Dispatch (`syscall/mod.rs`)

```rust
pub fn syscall(num: usize, args: [usize; 6]) -> isize {
    match num {
        SYS_FORK => sys_fork(),
        SYS_EXIT => sys_exit(args[0] as i32),
        SYS_WAIT => sys_wait(args[0] as *mut i32),
        SYS_PIPE => sys_pipe(args[0] as *mut [i32; 2]),
        // ... all 22 syscalls
        _ => -1,
    }
}
```

### Syscall Numbers (matching C xv6)

| Number | Syscall | Args |
|--------|---------|------|
| 1 | fork | - |
| 2 | exit | int status |
| 3 | wait | int* status |
| 4 | pipe | int[2] fds |
| 5 | read | int fd, void* buf, int n |
| 6 | write | int fd, void* buf, int n |
| 7 | close | int fd |
| 8 | kill | int pid |
| 9 | exec | char* path, char** argv |
| 10 | fstat | int fd, struct stat* |
| 11 | chdir | char* path |
| 12 | dup | int fd |
| 13 | getpid | - |
| 14 | sbrk | int n |
| 15 | sleep | int ticks |
| 16 | uptime | - |
| 17 | open | char* path, int flags |
| 18 | mknod | char* path, short major, short minor |
| 19 | unlink | char* path |
| 20 | link | char* old, char* new |
| 21 | mkdir | char* path |

### User Space Interface (`user-lib/src/syscall.rs`)

```rust
macro_rules! syscall {
    ($num:expr) => { ... };
    ($num:expr, $a0:expr) => { ... };
    // ... up to 6 args
}
```

---

## User Space

### Programs (`user/src/bin/`)

Each program is a separate binary:
- `sh.rs` - shell
- `ls.rs` - list directory
- `cat.rs` - concatenate files
- `init.rs` - init process
- `usertests.rs` - test suite

### User Library (`user-lib/src/`)

- `syscall.rs` - syscall macros and wrappers
- `stdio.rs` - `print!`, `println!`, `read`, `write`
- `string.rs` - `strcmp`, `strcpy`, `strlen`, etc.
- `fs.rs` - `open`, `close`, `read`, `write`, `mkdir`, etc.
- `process.rs` - `fork`, `exec`, `wait`, `exit`, `getpid`

### Building User Programs

- Target: `riscv64gc-unknown-linux-gnu` (static)
- Linked with `user-lib` and `alloc`
- Heap initialized via `sbrk` syscall

---

## Build System

### Workspace (`Cargo.toml`)

```toml
[workspace]
members = ["kernel", "user", "user-lib", "xtask"]
resolver = "2"
```

### Kernel Crate (`kernel/Cargo.toml`)

```toml
[package]
name = "kernel"
edition = "2021"

[dependencies]
riscv = "0.11"
tock-registers = "0.8"
linked-list-allocator = "0.11"
spin = "0.9"
alloc = { version = "1", features = ["alloc_error_handler"] }
```

- `no_std`, `no_main`
- Target: `riscv64imac-unknown-none-elf`
- Linker script: `memory.x`
- Build script: `build.rs` for assembly

### User Crate (`user/Cargo.toml`)

```toml
[package]
name = "user"
edition = "2021"

[dependencies]
user-lib = { path = "../user-lib" }
```

- `std` available
- Target: `riscv64gc-unknown-linux-gnu`
- Each `src/bin/*.rs` becomes a separate binary

### Build Automation (`xtask/`)

Custom build tasks for:
- Creating `fs.img` with user programs
- Running QEMU with correct arguments
- Integration testing

### Running in QEMU

```bash
qemu-system-riscv64 \
    -machine virt \
    -nographic \
    -bios none \
    -kernel kernel/target/riscv64imac-unknown-none-elf/release/kernel \
    -drive file=fs.img,if=none,format=raw,id=x0 \
    -device virtio-blk-device,drive=x0
```

---

## Testing

### Unit Tests
```bash
cargo test --package kernel
cargo test --package user-lib
```

### Integration Tests
```bash
cargo test --test integration
```
Runs kernel in QEMU, executes usertests, compares output.

---

## Performance Goals

| Metric | Target |
|--------|--------|
| Boot time | ≤ C version |
| Syscall latency | ≤ C version |
| Context switch | ≤ C version (same asm) |
| Memory overhead | Minimal (no GC) |

---

## References

- [xv6 Book](https://pdos.csail.mit.edu/6.1810/) - MIT 6.1810 course materials
- [RISC-V Privileged Spec](https://riscv.org/technical/specifications/) - ISA specification
- [Rust Embedded Book](https://docs.rust-embedded.org/book/) - Rust for embedded/kernel