# xv6-riscv Rust Port Architecture

## Scope and status

This repository contains two implementations of the xv6-riscv teaching operating system:

- the original C xv6 reference implementation in `kernel/` and `user/`; and
- an in-progress Rust migration in `kernel/src/`, `user/src/`, and `user-lib/src/`.

The Rust tree follows xv6 subsystem boundaries and ABI conventions where the source implements them. It is not a documented completed replacement, and this document does not claim binary or disk compatibility, test-suite success, or performance equivalence. The C tree remains the behavioural reference and is also the system built by the root `Makefile`.

The status labels below are statements about checked-in source, not feature-parity guarantees:

- **Ported structure**: a Rust module and its principal C xv6 counterpart are present.
- **Implemented path**: the named runtime path exists in the Rust source.
- **Migration limitation**: completeness or compatibility requires further implementation or validation.

## Repository layout and source correspondence

| Responsibility | Rust migration | C xv6 reference |
| --- | --- | --- |
| Kernel entry, CPU setup, traps | `kernel/src/arch/` | `kernel/entry.S`, `kernel/start.c`, `kernel/trap.c`, `kernel/trampoline.S` |
| Virtual memory and allocation | `kernel/src/mm/` | `kernel/vm.c`, `kernel/kalloc.c` |
| Processes and scheduling | `kernel/src/proc/` | `kernel/proc.c`, `kernel/swtch.S` |
| File system and descriptors | `kernel/src/fs/` | `kernel/bio.c`, `kernel/fs.c`, `kernel/log.c`, `kernel/file.c`, `kernel/pipe.c` |
| Devices and interrupts | `kernel/src/drivers/`, `arch/interrupt.rs` | `kernel/console.c`, `kernel/uart.c`, `kernel/virtio_disk.c`, `kernel/plic.c` |
| System calls | `kernel/src/proc/syscall.rs` | `kernel/syscall.c`, `kernel/sysproc.c`, `kernel/sysfile.c` |
| ELF loading | `kernel/src/elf.rs` | `kernel/exec.c` |
| User runtime and programs | `user-lib/src/`, `user/src/bin/` | `user/ulib.c`, `usys.pl`, `user/*.c` |
| Image construction | Rust build scripts plus `mkfs/` | root `Makefile` plus `mkfs/` |

`kernel/src/lib.rs` exports the Rust kernel's `arch`, `mm`, `sync`, `proc`, `fs`, `drivers`, `syscall`, `trap`, and `elf` modules. The Cargo workspace has three members: `kernel`, `user`, and `user-lib`.

## Boot and privilege transitions

**Ported structure:** `kernel/src/arch/entry.S`, `asm.S`, `init.rs`, `asm.rs`, and `trap.rs` correspond to C xv6's `entry.S`, `start.c`, `trap.c`, and `trampoline.S`. `asm.S` implements the `uservec`, `userret`, and `kernelvec` assembly entry points; `asm.rs` provides CSR and address helpers used by Rust code.

**Implemented path:** the entry assembly reaches `mstart()`, which configures machine-mode state, delegates exceptions and interrupts to supervisor mode, enables supervisor timer support, stores the hart ID in `tp`, and uses `mret` to enter `init()`. Hart 0 initializes the console, frame allocator, kernel page table, process table, traps, PLIC, VirtIO device, and file-system caches before creating the first user process. Other harts wait for this initialization, then set up their page tables, traps, and PLIC state.

`kernel/src/elf.rs` embeds `user/_init` with `include_bytes!`. Thus the first user process runs the copy embedded in the kernel image rather than the copy in `fs.img`; changing Rust `init` requires rebuilding the user binary before rebuilding the kernel.

**Migration limitation:** the README's development command boots one hart while SMP bring-up is in progress. Multi-hart correctness must be validated separately.

## Memory and address spaces

**Ported structure:** `kernel/src/mm/` and `kernel/src/arch/paging.rs` map to C xv6's `vm.c` and `kalloc.c`.

- `mm/address.rs` defines typed physical and virtual addresses and page numbers: `PhysAddr`, `VirtAddr`, `PhysPageNum`, and `VirtPageNum`.
- `mm/frame_allocator.rs` provides the physical frame allocator and its `kinit`, `kalloc`, and `kfree` entry points.
- `mm/page_table.rs` implements Sv39 three-level page tables with 4 KiB pages, kernel mappings, user allocation/copy/free helpers, and a trampoline mapping.
- `mm/heap.rs` initializes a Rust kernel heap; `mm/page_fault.rs` contains a page-fault entry point.

`kernel/memory.x` defines a 128 MiB kernel memory region at `0x80000000`. User layout is established by page-table code, not by a separate `USER` linker-memory region. The trampoline is at `0xFFFF_FFFF_FFFF_F000`, with the trap-frame page directly below it, as defined in `arch/asm.rs`.

`PageTable` distinguishes owning trees from non-owning views created by `Clone` or `from_root`. Only an owning handle frees the tree. The teardown code clears trampoline and trap-frame mappings without freeing their backing pages.

**Migration limitation:** Rust code still uses raw pointers and `unsafe` at hardware, assembly, page-table, and shared-kernel boundaries. Rust types help document some boundaries, but no blanket memory-safety claim follows.

## Processes, scheduling, and synchronization

**Ported structure:** `kernel/src/proc/` and `kernel/src/sync/` correspond to the process, scheduler, context-switch, spinlock, and sleeplock portions of C xv6's `proc.c`, `swtch.S`, `spinlock.c`, and `sleeplock.c`.

`Proc` contains a `SpinLock<ProcInner>`. `ProcInner` stores the xv6-style process state, PID, parent pointer, user page table, trap frame, kernel context, file descriptors, current working directory, exit state, and memory size. Its state model is:

```text
Unused → Used → Runnable → Running → Sleeping → Runnable → … → Zombie → Unused
```

**Implemented path:** `proc/scheduler.rs` allocates process slots and kernel stacks, scans runnable processes, and switches through assembly `swtch`. `proc/mod.rs` provides `userinit`, `sleep`, `wakeup`, `kexit`, timer ticks, and current CPU/process accessors. `arch/trap.rs` defines trap frames and contexts; `usertrapret` prepares a trap frame and transfers through the trampoline back to user mode.

`sync/spinlock.rs` implements interrupt-aware spin locks and `push_off` / `pop_off`. `sync/sleeplock.rs`, `sync/condvar.rs`, and `sync/mutex.rs` provide the other synchronization primitives. Scheduler and sleep paths follow xv6 lock-across-switch discipline where the source requires it; for example, `forkret` releases the process lock after the first switch into a process.

**Migration limitation:** process state combines Rust ownership, `Arc<File>`, raw process pointers, raw trap-frame pointers, and raw inode pointers. Their lifetime rules depend on kernel locking and control flow rather than being fully encoded in safe Rust types.

## File system and descriptors

**Ported structure:** `kernel/src/fs/` corresponds to C xv6's buffer-cache, inode/path, log, file, and pipe layers.

| Rust module | Implemented path | C xv6 counterpart |
| --- | --- | --- |
| `fs/buf.rs` | 1 KiB buffer cache, block reads/writes, pinning | `bio.c` |
| `fs/inode.rs` | inode cache, allocation, directories, `namei` / `nameiparent` | `fs.c` |
| `fs/log.rs` | `begin_op`, `end_op`, log recovery | `log.c` |
| `fs/file.rs` | open-file table and descriptor operations | `file.c` |
| `fs/pipe.rs` | pipe state and endpoints | `pipe.c` |

`fsinit()` initializes the buffer and inode caches, reads the superblock, initializes the log, and runs log recovery. Processes retain open files in a fixed-size descriptor array and track their current working directory for filesystem calls.

**Migration limitation:** the on-disk constants and structures are represented in Rust, but C xv6 image compatibility and failure-mode parity require testing. `mkfs/mkfs.c` remains a C host-side program that produces `fs.img`.

## Devices and interrupts

**Ported structure:** Rust device support is split between `kernel/src/drivers/` and `kernel/src/arch/interrupt.rs`; it corresponds to C xv6's console, UART, VirtIO, and PLIC code.

- `drivers/uart.rs` implements the 16550-compatible UART at `0x10000000`.
- `drivers/console.rs` provides console input/output and connects console reads and writes to file descriptors.
- `drivers/virtio.rs` initializes and drives the VirtIO MMIO block device at `0x10001000`.
- `arch/interrupt.rs` configures and services the PLIC, dispatches UART and VirtIO interrupts, and programs supervisor timer interrupts.

**Implemented path:** user and kernel traps are handled in `arch/trap.rs`. User traps save state, route `ecall` to the process syscall dispatcher, handle device interrupts, and return through the trampoline. Kernel traps use `kernelvec` while executing with the kernel page table.

## System calls and user space

**Implemented path:** syscall numbers 1 through 21 are defined in `kernel/src/proc/syscall.rs` and mirrored in `user-lib/src/lib.rs`. The Rust entry point is `proc_syscall()`: it reads the number and arguments from the current process trap frame, dispatches to `sys_*` functions, and writes the result back to that frame. This corresponds to C xv6's `syscall.c`, `sysproc.c`, and `sysfile.c`.

The represented syscall surface is `fork`, `exit`, `wait`, `pipe`, `read`, `write`, `close`, `kill`, `exec`, `fstat`, `chdir`, `dup`, `getpid`, `sbrk`, `sleep`, `uptime`, `open`, `mknod`, `unlink`, `link`, and `mkdir`.

`user-lib/src/` provides syscall wrappers plus minimal stdio, string, file, and process helpers. `user/src/bin/` contains Rust `sh`, `ls`, `cat`, `init`, `init_test`, `echo`, `mkdir`, and `rm`. The Rust shell parses and runs commands, pipes, redirections, lists, and subshells; its C counterpart is `user/sh.c`.

**Migration limitation:** `user/Cargo.toml` retains an in-package `[build]` entry for `riscv64gc-unknown-linux-gnu`, but the active Rust build scripts explicitly pass `--target riscv64imac-unknown-none-elf` for both user programs and the kernel. The latter is the current scripted build path; the manifest entry is stale metadata that should be reconciled.

## Build and image assembly

The repository has distinct C-reference and Rust-port build paths.

### C xv6 reference

The root `Makefile` compiles `kernel/*.c` and assembly files, builds C user programs, invokes the C `mkfs` tool, and starts QEMU with `make qemu`. It does not build the Rust kernel.

### Rust migration

The workspace manifests describe `xv6-kernel`, `xv6-user`, and `xv6-user-lib`. `build_rust_users.sh` explicitly builds Rust user programs and the kernel for `riscv64imac-unknown-none-elf`, then packages `fs.img`. `run_usertests.sh` invokes that build path and boots QEMU. `.cargo/config.toml` supplies linker and Rust-flag settings for that target but declares no default target or QEMU runner.

The user binaries must be built before the kernel when `init` changes, because the kernel embeds `user/_init`. The filesystem image is then assembled with the C `mkfs` program. The scripts and README are the executable build contract for current command-line details.

## Verification and limitations

This document is a source map, not a compatibility certificate. It records the subsystem structures and execution paths present in the repository. It deliberately does not claim complete xv6 parity, disk-image compatibility, test-suite success, or performance equivalence.

When extending the Rust port, update this document only after checking the corresponding Rust path, the C xv6 reference path, and the build flow that packages and boots the changed code.
