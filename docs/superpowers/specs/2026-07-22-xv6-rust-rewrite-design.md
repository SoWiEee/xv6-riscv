# xv6-riscv Rust Rewrite Design

## Goal
Complete rewrite of xv6-riscv teaching OS kernel in Rust, leveraging Rust's memory safety, ownership model, and zero-cost abstractions to eliminate C memory bugs while maintaining identical behavior and performance characteristics.

## Architecture

### Language Choice: Rust over Go
- **No runtime**: Rust has no GC/runtime, essential for kernel development
- **Memory control**: Direct control over memory layout, allocation, and page tables
- **Borrow checker**: Compile-time prevention of use-after-free, double-free, data races
- **Embedded ecosystem**: `embedded-hal`, `riscv`, `tock-registers` crates provide hardware abstraction
- **Existing OS kernels**: Theseus, Redox, Tock, hubris prove Rust viability for kernels

### High-Level Structure
```
xv6-rust/
├── kernel/                 # Kernel crate (no_std)
│   ├── src/
│   │   ├── arch/           # RISC-V architecture specifics
│   │   │   ├── registers.rs    # CSR access via tock-registers
│   │   │   ├── trap.rs         # Trap handling, trampoline
│   │   │   ├── paging.rs       # Sv39 page table management
│   │   │   ├── interrupt.rs    # PLIC, timer, UART
│   │   │   └── asm.rs          # Inline assembly (context switch, etc.)
│   │   ├── mm/             # Memory management
│   │   │   ├── frame_allocator.rs  # Physical page allocator (kalloc/kfree)
│   │   │   ├── page_table.rs       # Page table walk, map/unmap
│   │   │   ├── address.rs          # Virtual/Physical address types
│   │   │   └── heap.rs             # Kernel heap allocator
│   │   ├── fs/             # File system
│   │   │   ├── buf.rs            # Buffer cache
│   │   │   ├── inode.rs          # Inode management
│   │   │   ├── log.rs            # Write-ahead logging
│   │   │   └── file.rs           # File descriptor layer
│   │   ├── proc/           # Process management
│   │   │   ├── process.rs        # Process struct, state machine
│   │   │   ├── scheduler.rs      # Round-robin scheduler
│   │   │   ├── trapframe.rs      # Trap frame layout
│   │   │   ├── context.rs        # Kernel context switch
│   │   │   └── syscall.rs        # System call dispatch
│   │   ├── sync/           # Synchronization primitives
│   │   │   ├── spinlock.rs       # Spinlock with IRQ save/restore
│   │   │   ├── sleeplock.rs      # Sleeping lock
│   │   │   └── condvar.rs        # Wait queues (sleep/wakeup)
│   │   ├── drivers/        # Device drivers
│   │   │   ├── uart.rs           # 16550 UART
│   │   │   ├── virtio.rs         # Virtio block device
│   │   │   └── console.rs        # Console output
│   │   ├── main.rs         # Kernel entry, initialization
│   │   ├── panic.rs        # Panic handler
│   │   └── lib.rs          # Crate root, exports
│   ├── Cargo.toml
│   └── memory.x            # Linker script for memory layout
├── user/                   # User-space programs (std)
│   ├── src/bin/            # Each program as binary
│   │   ├── sh.rs
│   │   ├── ls.rs
│   │   ├── cat.rs
│   │   └── ...
│   ├── src/lib.rs          # User library (ulib equivalent)
│   └── Cargo.toml
├── .cargo/
│   └── config.toml         # Target specification, runner
├── build.rs                # Build script for asm, linker
├── xtask/                  # Build automation
└── Cargo.toml              # Workspace root
```

## Key Design Decisions

### 1. Memory Safety Without Compromise
- **No `unsafe` in safe abstractions**: All unsafe code isolated in `arch/`, `mm/frame_allocator`, `sync/`
- **Typed addresses**: `PhysAddr`, `VirtAddr`, `PhysPageNum`, `VirtPageNum` newtypes prevent mixing
- **Page table as RAII**: `PageTable` owns mappings, drops free pages automatically
- **Capability-based access**: `UserBuffer` for copyin/copyout with bounds checking

### 2. Concurrency Model
- **Spinlock with guard pattern**: `SpinLock<T>` returns `SpinLockGuard<'_, T>` implementing `Deref`
- **No lock held across await**: All kernel code sync, no async in kernel
- **Per-CPU data**: `CpuLocal<T>` using `#[thread_local]` or percpu crate
- **Interrupt safety**: `IrqSafeSpinLock` disables interrupts on acquire

### 3. Trap Handling
- **Trampoline in Rust**: Inline asm for uservec/kernelvec, Rust for trap logic
- **TrapFrame as struct**: `#[repr(C, align(16))]` matching RISC-V ABI
- **Context switch**: `context_switch(old: &mut Context, new: &Context)` in asm

### 4. File System
- **Buffer cache**: `Arc<Mutex<Buf>>` with LRU eviction
- **Inode reference counting**: `Arc<Inode>` with `weak` for parent directory
- **Logging**: Transaction API `log::begin_op()`, `log::end_op()`

### 5. Process Management
- **Process state machine**: `enum ProcState { Unused, Used, Sleeping, Runnable, Running, Zombie }`
- **Scheduler**: Simple round-robin, `yield()` switches context
- **PID allocator**: Bitmap or atomic counter

### 6. User Space
- **Separate crate**: `user` crate with `std`, compiles to RISC-V binaries
- **Syscall interface**: `syscall!(nr, args...)` macro generating `ecall`
- **User library**: `read`, `write`, `open`, `close`, `fork`, `exec`, `wait`, `exit`

## Component Specifications

### Memory Layout (memory.x)
```
MEMORY
{
  KERNEL : ORIGIN = 0x80000000, LENGTH = 128M
  USER   : ORIGIN = 0x00000000, LENGTH = 2G   (virtual, per-process)
}
```

### Page Table Constants (Sv39)
- Page size: 4096 bytes
- 3 levels, 512 entries each
- PTE: 64-bit, V=bit0, R=bit1, W=bit2, X=bit3, U=bit4, G=bit5, A=bit6, D=bit7

### Kernel Stack
- One page per process, mapped at top of kernel VA space
- Guard page below (unmapped) to catch overflow

### Interrupts
- PLIC: Memory-mapped, priority-based
- Timer: `stimecmp` CSR, `ssip` SIP bit
- UART: `UART0` at 0x10000000
- Virtio: `VIRTIO0` at 0x10001000

## Error Handling
- **Kernel**: `Result<T, KernelError>` for fallible operations, `panic!` for invariants
- **User**: Standard `Result<T, Errno>` with errno constants

## Testing Strategy
- **Unit tests**: `#[cfg(test)]` in each module, run with `cargo test`
- **Integration tests**: `qemu` + `expect` scripts in `tests/`
- **Property tests**: `proptest` for page table, allocator
- **Comparison tests**: Run xv6 C tests, compare output

## Build System
- **Workspace**: Two crates (`kernel` no_std, `user` std)
- **Target**: `riscv64imac-unknown-none-elf` (kernel), `riscv64gc-unknown-linux-gnu` (user)
- **Runner**: `qemu-system-riscv64 -machine virt -nographic -bios none -kernel`
- **Linker**: `ld.lld` with custom linker script

## Migration Strategy (Incremental)
1. **Phase 1**: Arch primitives (registers, paging, traps, context switch)
2. **Phase 2**: Memory allocator, page table, heap
3. **Phase 3**: Synchronization (spinlock, sleeplock, condvar)
4. **Phase 4**: Drivers (UART, virtio, console, PLIC)
5. **Phase 5**: Process management, scheduler, syscalls
6. **Phase 6**: File system (buf, log, inode, file)
6. **Phase 7**: User space programs, syscall interface
7. **Phase 8**: Integration testing, all usertests pass

## Performance Goals
- **Boot time**: ≤ C version
- **Syscall latency**: ≤ C version (no bounds check overhead in hot paths)
- **Context switch**: ≤ C version (same asm)
- **Memory overhead**: Minimal (no GC, thin abstractions)

## Compatibility
- **Binary compatible**: Same syscall numbers, same ELF format for user programs
- **Disk compatible**: Same FS layout (can mount C xv6 fs.img)
- **Test compatible**: All existing usertests pass unchanged