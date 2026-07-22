# xv6-riscv Rust Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete rewrite of xv6-riscv kernel in Rust with memory safety, identical behavior, and same test compatibility.

**Architecture:** Rust workspace with `kernel` (no_std) and `user` (std) crates. Incremental migration by subsystem.

**Tech Stack:** Rust 2024, `riscv` crate for CSR access, `tock-registers` for register definitions, `linked-list-allocator` for heap, `qemu-system-riscv64` for testing.

## Global Constraints

- Target: `riscv64imac-unknown-none-elf` (kernel), `riscv64gc-unknown-linux-gnu` (user)
- No `std` in kernel crate; `alloc` only with custom global allocator
- All unsafe code isolated in `arch/`, `mm/frame_allocator`, `sync/`
- Same syscall ABI, same FS layout, same test suite
- Must boot in QEMU and pass all `usertests`

---

### Task 1: Project Setup & Build Infrastructure

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `kernel/Cargo.toml`
- Create: `kernel/memory.x`
- Create: `kernel/build.rs`
- Create: `.cargo/config.toml`
- Create: `kernel/src/lib.rs`
- Create: `kernel/src/arch/asm.rs`
- Create: `kernel/src/panic.rs`
- Create: `user/Cargo.toml`
- Create: `user/src/lib.rs`

**Interfaces:**
- Produces: Buildable workspace, QEMU runner, panic handler, basic asm primitives

- [ ] **Step 1.1: Create workspace Cargo.toml**

```toml
# Cargo.toml
[workspace]
resolver = "2"
members = ["kernel", "user"]

[workspace.dependencies]
riscv = { version = "0.12", features = ["inline-asm"] }
tock-registers = "0.8"
bitflags = "2"
spin = "0.9"
linked-list-allocator = "0.11"
buddy-system-allocator = "0.6"
x86_64 = { version = "0.17", optional = true }  # for testing on x86
```

- [ ] **Step 1.2: Create kernel/Cargo.toml**

```toml
# kernel/Cargo.toml
[package]
name = "xv6-kernel"
version = "0.1.0"
edition = "2024"

[dependencies]
riscv = { workspace = true, features = ["inline-asm"] }
tock-registers = { workspace = true }
bitflags = { workspace = true }
spin = { workspace = true }
linked-list-allocator = { workspace = true }
buddy-system-allocator = { workspace = true }

[profile.release]
panic = "abort"
lto = true
codegen-units = 1
strip = true
opt-level = "z"

[profile.dev]
panic = "abort"

[target.riscv64imac-unknown-none-elf]
runner = "qemu-system-riscv64 -machine virt -nographic -bios none -kernel"
```

- [ ] **Step 1.3: Create kernel/memory.x (linker script)**

```ld
/* kernel/memory.x */
MEMORY
{
    KERNEL : ORIGIN = 0x80000000, LENGTH = 128M
}

ENTRY(_start)

SECTIONS
{
    . = ORIGIN(KERNEL);
    .text : {
        *(.text.entry)
        *(.text .text.*)
        *(.rodata .rodata.*)
    }
    .data : {
        *(.data .data.*)
        *(.bss .bss.*)
        *(.sbss .sbss.*)
    }
    .stack (NOLOAD) : {
        . = ALIGN(16);
        _stack_start = .;
        . += 4096;
        _stack_end = .;
    }
    _kernel_end = .;
}
```

- [ ] **Step 1.4: Create .cargo/config.toml**

```toml
# .cargo/config.toml
[build]
target = "riscv64imac-unknown-none-elf"

[target.riscv64imac-unknown-none-elf]
linker = "rust-lld"
rustflags = [
    "-C", "link-arg=-Tkernel/memory.x",
    "-C", "link-arg=-zmax-page-size=4096",
    "-C", "default-linker-libraries",
]

[unstable]
build-std = ["core", "compiler_builtins", "alloc"]
build-std-features = ["compiler-builtins-mem"]
```

- [ ] **Step 1.5: Create kernel/build.rs**

```rust
// kernel/build.rs
fn main() {
    println!("cargo:rustc-link-arg=-Tkernel/memory.x");
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=src/arch/asm.S");
    
    // Compile assembly files
    cc::Build::new()
        .file("src/arch/asm.S")
        .compile("xv6asm");
}
```

- [ ] **Step 1.6: Create kernel/src/lib.rs**

```rust
// kernel/src/lib.rs
#![no_std]
#![no_main]
#![feature(
    asm_const,
    asm_experimental_arch,
    naked_functions,
    alloc_error_handler,
    panic_info_message,
    lang_items,
    const_mut_refs,
    ptr_metadata,
    strict_provenance
)]

extern crate alloc;

pub mod arch;
pub mod mm;
pub mod sync;
pub mod proc;
pub mod fs;
pub mod drivers;
pub mod syscall;
pub mod trap;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::console::printk!("KERNEL PANIC: {}\n", info);
    loop {
        riscv::asm::wfi();
    }
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Allocation failed: {:?}", layout);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    arch::init::init();
}
```

- [ ] **Step 1.7: Create kernel/src/arch/asm.rs (inline asm primitives)**

```rust
// kernel/src/arch/asm.rs
use core::arch::asm;

/// Read a CSR
#[inline]
pub fn read_csr(csr: usize) -> usize {
    let val: usize;
    unsafe { asm!("csrr {}, {}", out(reg) val, const csr) };
    val
}

/// Write a CSR
#[inline]
pub fn write_csr(csr: usize, val: usize) {
    unsafe { asm!("csrw {}, {}", const csr, in(reg) val) };
}

/// Read and write CSR atomically
#[inline]
pub fn swap_csr(csr: usize, val: usize) -> usize {
    let old: usize;
    unsafe { asm!("csrrw {}, {}, {}", out(reg) old, const csr, in(reg) val) };
    old
}

/// Set bits in CSR
#[inline]
pub fn set_csr(csr: usize, val: usize) {
    unsafe { asm!("csrs {}, {}", const csr, in(reg) val) };
}

/// Clear bits in CSR
#[inline]
pub fn clear_csr(csr: usize, val: usize) {
    unsafe { asm!("csrc {}, {}", const csr, in(reg) val) };
}

/// Read SSTATUS
#[inline]
pub fn r_sstatus() -> usize {
    read_csr(0x100)
}

/// Write SSTATUS
#[inline]
pub fn w_sstatus(val: usize) {
    write_csr(0x100, val);
}

/// Read SEPC
#[inline]
pub fn r_sepc() -> usize {
    read_csr(0x141)
}

/// Write SEPC
#[inline]
pub fn w_sepc(val: usize) {
    write_csr(0x141, val);
}

/// Read SCAUSE
#[inline]
pub fn r_scause() -> usize {
    read_csr(0x142)
}

/// Read STVAL
#[inline]
pub fn r_stval() -> usize {
    read_csr(0x143)
}

/// Read SATP
#[inline]
pub fn r_satp() -> usize {
    read_csr(0x180)
}

/// Write SATP
#[inline]
pub fn w_satp(val: usize) {
    write_csr(0x180, val);
}

/// Read STVEC
#[inline]
pub fn r_stvec() -> usize {
    read_csr(0x105)
}

/// Write STVEC
#[inline]
pub fn w_stvec(val: usize) {
    write_csr(0x105, val);
}

/// Read SIE
#[inline]
pub fn r_sie() -> usize {
    read_csr(0x104)
}

/// Write SIE
#[inline]
pub fn w_sie(val: usize) {
    write_csr(0x104, val);
}

/// Read SIP
#[inline]
pub fn r_sip() -> usize {
    read_csr(0x144)
}

/// Read STP (thread pointer / hartid)
#[inline]
pub fn r_tp() -> usize {
    let val: usize;
    unsafe { asm!("mv {}, tp", out(reg) val) };
    val
}

/// Read TIME
#[inline]
pub fn r_time() -> usize {
    read_csr(0xC01)
}

/// Write STIMECMP
#[inline]
pub fn w_stimecmp(val: usize) {
    write_csr(0x14D, val);
}

/// Enable interrupts
#[inline]
pub fn intr_on() {
    unsafe { asm!("csrs sstatus, {}", const (1 << 1)) }; // SIE bit
}

/// Disable interrupts
#[inline]
pub fn intr_off() {
    unsafe { asm!("csrc sstatus, {}", const (1 << 1)) };
}

/// Check if interrupts enabled
#[inline]
pub fn intr_get() -> bool {
    (r_sstatus() & (1 << 1)) != 0
}

/// Supervisor fence.vma
#[inline]
pub fn sfence_vma() {
    unsafe { asm!("sfence.vma") };
}

/// Wait for interrupt
#[inline]
pub fn wfi() {
    unsafe { asm!("wfi") };
}

/// ECALL instruction
#[inline]
pub fn ecall() {
    unsafe { asm!("ecall") };
}

/// EBREAK instruction
#[inline]
pub fn ebreak() {
    unsafe { asm!("ebreak") };
}
```

- [ ] **Step 1.8: Create kernel/src/panic.rs (already in lib.rs, skip if done)**

- [ ] **Step 1.9: Create user/Cargo.toml**

```toml
# user/Cargo.toml
[package]
name = "xv6-user"
version = "0.1.0"
edition = "2024"

[dependencies]
xv6-user-lib = { path = "../user-lib" }

[[bin]]
name = "sh"
path = "src/bin/sh.rs"

[[bin]]
name = "ls"
path = "src/bin/ls.rs"

# ... add all user programs
```

- [ ] **Step 1.10: Create user-lib crate**

```toml
# user-lib/Cargo.toml
[package]
name = "xv6-user-lib"
version = "0.1.0"
edition = "2024"

[dependencies]
```

```rust
// user-lib/src/lib.rs
#![no_std]
extern crate alloc;

pub mod syscall;
pub mod stdio;
pub mod string;
pub mod fs;
pub mod process;

// Syscall numbers matching xv6
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
pub const SYS_WRITE: usize = 18;
pub const SYS_MKNOD: usize = 19;
pub const SYS_UNLINK: usize = 20;
pub const SYS_LINK: usize = 21;
pub const SYS_MKDIR: usize = 22;
pub const SYS_CLOSE: usize = 23;

// Syscall macro
#[macro_export]
macro_rules! syscall {
    ($num:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    ($num:expr, $a0:expr) => {{
        let ret: usize;
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") $num,
                in("a0") $a0,
                lateout("a0") ret,
                options(nostack)
            );
        }
        ret
    }};
    // ... more overloads for 2-6 args
}
```

- [ ] **Step 1.11: Verify build**

Run: `cargo build --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Compiles, produces `target/riscv64imac-unknown-none-elf/debug/xv6-kernel`

- [ ] **Step 1.12: Test QEMU boot**

Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: QEMU starts, prints "KERNEL PANIC" (since init not implemented), loops on WFI

- [ ] **Step 1.13: Commit**

```bash
git add Cargo.toml kernel/ user/ .cargo/
git commit -m "chore: project setup, build infrastructure, panic handler"
```

---

### Task 2: RISC-V Architecture Primitives

**Files:**
- Create: `kernel/src/arch/registers.rs`
- Create: `kernel/src/arch/paging.rs`
- Create: `kernel/src/arch/trap.rs`
- Create: `kernel/src/arch/interrupt.rs`
- Create: `kernel/src/arch/init.rs`

**Interfaces:**
- Consumes: asm primitives from Task 1
- Produces: CSR definitions, page table types, trap frame, interrupt controller

- [ ] **Step 2.1: Create kernel/src/arch/registers.rs (tock-registers definitions)**

```rust
// kernel/src/arch/registers.rs
use tock_registers::{register_bitfields, register_structs, registers::*};

register_structs! {
    #[allow(non_snake_case)]
    pub Plic {
        pub priority: [ReadWrite<u32>; 1024],
        _reserved0: [u32; 1024],
        pub pending: [ReadOnly<u32>; 32],
        _reserved1: [u32; 992],
        pub enable: [ReadWrite<u32>; 15872],
        _reserved2: [u32; 15872],
        pub threshold: [ReadWrite<u32>; 15872],
        _reserved3: [u32; 15872],
        pub claim: [ReadWrite<u32>; 15872],
    }
}

register_bitfields! [u32,
    SSTATUS [
        SPP OFFSET(8) NUMBITS(1) [],  // Supervisor Previous Privilege
        SPIE OFFSET(5) NUMBITS(1) [], // Supervisor Previous Interrupt Enable
        SIE OFFSET(1) NUMBITS(1) [],  // Supervisor Interrupt Enable
    ],
    SCAUSE [
        INTERRUPT OFFSET(63) NUMBITS(1) [],
        EXCEPTION_CODE OFFSET(0) NUMBITS(63) [],
    ],
    SATP [
        MODE OFFSET(60) NUMBITS(4) [
            Bare = 0,
            Sv39 = 8,
            Sv48 = 9,
            Sv57 = 10,
        ],
        ASID OFFSET(44) NUMBITS(16) [],
        PPN OFFSET(0) NUMBITS(44) [],
    ],
    PTE [
        V OFFSET(0) NUMBITS(1) [],
        R OFFSET(1) NUMBITS(1) [],
        W OFFSET(2) NUMBITS(1) [],
        X OFFSET(3) NUMBITS(1) [],
        U OFFSET(4) NUMBITS(1) [],
        G OFFSET(5) NUMBITS(1) [],
        A OFFSET(6) NUMBITS(1) [],
        D OFFSET(7) NUMBITS(1) [],
        RSW OFFSET(8) NUMBITS(2) [],
        PPN0 OFFSET(10) NUMBITS(9) [],
        PPN1 OFFSET(19) NUMBITS(9) [],
        PPN2 OFFSET(28) NUMBITS(26) [],
    ],
]

pub const PTE_V: u64 = 1 << 0;
pub const PTE_R: u64 = 1 << 1;
pub const PTE_W: u64 = 1 << 2;
pub const PTE_X: u64 = 1 << 3;
pub const PTE_U: u64 = 1 << 4;
pub const PTE_G: u64 = 1 << 5;
pub const PTE_A: u64 = 1 << 6;
pub const PTE_D: u64 = 1 << 7;

pub const PGSIZE: usize = 4096;
pub const PGROUNDUP: usize = |x| (x + PGSIZE - 1) & !(PGSIZE - 1);
pub const PGROUNDDOWN: usize = |x| x & !(PGSIZE - 1);
```

- [ ] **Step 2.2: Create kernel/src/arch/paging.rs (Sv39 page table)**

```rust
// kernel/src/arch/paging.rs
use super::registers::{PTE_V, PTE_R, PTE_W, PTE_X, PTE_U, PGSIZE};
use super::asm::{sfence_vma, w_satp, MAKE_SATP};
use core::ptr::NonNull;

pub type PhysAddr = usize;
pub type VirtAddr = usize;
pub type PhysPageNum = usize;
pub type VirtPageNum = usize;

pub const PAGE_SIZE: usize = 4096;
pub const VPBITS: usize = 39;
pub const PPBITS: usize = 56;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn new() -> Self { Self(0) }
    pub fn is_valid(&self) -> bool { (self.0 & PTE_V) != 0 }
    pub fn ppn(&self) -> PhysPageNum { (self.0 >> 10) & ((1 << 44) - 1) }
    pub fn set_ppn(&mut self, ppn: PhysPageNum) { self.0 = (self.0 & 0x3FF) | ((ppn as u64) << 10) }
    pub fn flags(&self) -> u64 { self.0 & 0x3FF }
    pub fn set_flags(&mut self, flags: u64) { self.0 = (self.0 & !0x3FF) | flags }
    pub fn is_user(&self) -> bool { (self.0 & PTE_U) != 0 }
    pub fn is_writable(&self) -> bool { (self.0 & PTE_W) != 0 }
}

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self { entries: [PageTableEntry::new(); 512] }
    }

    pub fn as_mut_ptr(&mut self) -> *mut PageTableEntry {
        self.entries.as_mut_ptr()
    }

    pub fn get_mut(&mut self, vpn: VirtPageNum, level: usize) -> &mut PageTableEntry {
        &mut self.entries[vpn & 0x1FF]
    }
}

pub struct PageTableWalker {
    root: PhysPageNum,
}

impl PageTableWalker {
    pub fn new(root: PhysPageNum) -> Self { Self { root } }

    pub fn walk(&self, vaddr: VirtAddr, alloc: bool) -> Option<&mut PageTableEntry> {
        let mut pt = self.root;
        for level in (1..=2).rev() {
            let vpn = (vaddr >> (12 + level * 9)) & 0x1FF;
            let pte = unsafe { &mut *(Self::pte_ptr(pt, vpn) as *mut PageTableEntry) };
            if pte.is_valid() {
                pt = pte.ppn();
            } else if alloc {
                let new_pt = crate::mm::frame_allocator::alloc_page().ok()?;
                pte.set_ppn(new_pt);
                pte.set_flags(PTE_V);
                // Zero the new page table
                unsafe { core::ptr::write_bytes(Self::pte_ptr(new_pt, 0) as *mut u8, 0, PAGE_SIZE) };
                pt = new_pt;
            } else {
                return None;
            }
        }
        let vpn = (vaddr >> 12) & 0x1FF;
        Some(unsafe { &mut *(Self::pte_ptr(pt, vpn) as *mut PageTableEntry) })
    }

    fn pte_ptr(ppn: PhysPageNum, vpn: usize) -> usize {
        (ppn << 12) + (vpn * 8)
    }

    pub fn map_pages(&mut self, vaddr: VirtAddr, paddr: PhysAddr, pages: usize, flags: u64) -> Result<(), &'static str> {
        for i in 0..pages {
            let pte = self.walk(vaddr + i * PAGE_SIZE, true).ok_or("walk failed")?;
            if pte.is_valid() {
                return Err("remap");
            }
            pte.set_ppn((paddr + i * PAGE_SIZE) >> 12);
            pte.set_flags(flags | PTE_V);
        }
        Ok(())
    }
}

pub fn kvm_init() -> PhysPageNum {
    let root = crate::mm::frame_allocator::alloc_page().expect("kvm_init: no memory");
    let mut walker = PageTableWalker::new(root);
    // Map UART, VIRTIO, PLIC, kernel text/data, trampoline, kernel stacks
    // ... implementation
    root
}

pub fn kvm_init_hart(root: PhysPageNum) {
    unsafe { w_satp(MAKE_SATP(root)) };
    sfence_vma();
}
```

- [ ] **Step 2.3: Create kernel/src/arch/trap.rs (trap frame, trap handling)**

```rust
// kernel/src/arch/trap.rs
use super::asm::*;
use core::arch::asm;

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub kernel_satp: usize,
    pub kernel_sp: usize,
    pub kernel_trap: usize,
    pub epc: usize,
    pub kernel_hartid: usize,
    pub ra: usize,
    pub sp: usize,
    pub gp: usize,
    pub tp: usize,
    pub t0: usize,
    pub t1: usize,
    pub t2: usize,
    pub s0: usize,
    pub s1: usize,
    pub a0: usize,
    pub a1: usize,
    pub a2: usize,
    pub a3: usize,
    pub a4: usize,
    pub a5: usize,
    pub a6: usize,
    pub a7: usize,
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
    pub t3: usize,
    pub t4: usize,
    pub t5: usize,
    pub t6: usize,
}

impl TrapFrame {
    pub const fn new() -> Self {
        Self {
            kernel_satp: 0, kernel_sp: 0, kernel_trap: 0, epc: 0,
            kernel_hartid: 0, ra: 0, sp: 0, gp: 0, tp: 0,
            t0: 0, t1: 0, t2: 0, s0: 0, s1: 0,
            a0: 0, a1: 0, a2: 0, a3: 0, a4: 0, a5: 0, a6: 0, a7: 0,
            s2: 0, s3: 0, s4: 0, s5: 0, s6: 0, s7: 0,
            s8: 0, s9: 0, s10: 0, s11: 0,
            t3: 0, t4: 0, t5: 0, t6: 0,
        }
    }
}

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
    pub const fn new() -> Self {
        Self { ra: 0, sp: 0, s0: 0, s1: 0, s2: 0, s3: 0,
               s4: 0, s5: 0, s6: 0, s7: 0, s8: 0, s9: 0,
               s10: 0, s11: 0 }
    }
}

extern "C" {
    fn uservec();
    fn userret();
    fn kernelvec();
    fn swtch(old: *mut Context, new: *const Context);
}

pub fn context_switch(old: &mut Context, new: &Context) {
    unsafe { swtch(old as *mut Context, new as *const Context) }
}

pub fn prepare_return(tf: &mut TrapFrame) {
    intr_off();
    let trampoline_uservec = TRAMPOLINE + (uservec as usize - TRAMPOLINE);
    w_stvec(trampoline_uservec);
    tf.kernel_satp = r_satp();
    tf.kernel_sp = tf.kernel_sp; // set by caller
    tf.kernel_trap = usertrap as usize;
    tf.kernel_hartid = r_tp();
    let mut sstatus = r_sstatus();
    sstatus &= !SSTATUS_SPP;
    sstatus |= SSTATUS_SPIE;
    w_sstatus(sstatus);
    w_sepc(tf.epc);
}

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
    p.trapframe.epc = sepc;
    
    match scause {
        8 => { // syscall
            if crate::proc::is_killed(p) {
                crate::proc::kexit(-1);
            }
            p.trapframe.epc += 4;
            intr_on();
            crate::syscall::syscall();
        }
        scause if scause & (1 << 63) != 0 => { // interrupt
            let dev = crate::arch::interrupt::devintr();
            if dev == 2 { crate::proc::yield_now(); }
        }
        13 | 15 => { // page fault
            let read = scause == 13;
            if crate::mm::page_fault::handle_page_fault(p.pagetable, stval, read).is_err() {
                crate::proc::set_killed(p);
            }
        }
        _ => {
            crate::arch::console::printk!("usertrap: unexpected scause {:#x} pid={}\n", scause, p.pid);
            crate::proc::set_killed(p);
        }
    }
    
    if crate::proc::is_killed(p) {
        crate::proc::kexit(-1);
    }
    
    prepare_return(&mut p.trapframe);
    MAKE_SATP(p.pagetable)
}

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
        crate::arch::console::printk!("kerneltrap: scause={:#x} sepc={:#x} stval={:#x}\n", scause, r_sepc(), r_stval());
        panic!("kerneltrap");
    }
    
    if dev == 2 {
        if let Some(p) = crate::proc::current_process_opt() {
            crate::proc::yield_now();
        }
    }
    
    w_sepc(sepc);
    w_sstatus(sstatus);
}
```

- [ ] **Step 2.4: Create kernel/src/arch/interrupt.rs (PLIC, timer, UART)**

```rust
// kernel/src/arch/interrupt.rs
use super::registers::Plic;
use super::asm::*;

pub const UART0: usize = 0x10000000;
pub const VIRTIO0: usize = 0x10001000;
pub const PLIC_BASE: usize = 0x0C000000;
pub const UART0_IRQ: u32 = 10;
pub const VIRTIO0_IRQ: u32 = 1;

pub fn plic_init() {
    let plic = unsafe { &mut *(PLIC_BASE as *mut Plic) };
    // Set priorities
    for i in 1..=31 {
        plic.priority[i].write(1);
    }
    // Enable UART and Virtio for hart 0
    plic.enable[0].write((1 << UART0_IRQ) | (1 << VIRTIO0_IRQ));
    plic.threshold[0].write(0);
}

pub fn plic_init_hart() {
    let plic = unsafe { &mut *(PLIC_BASE as *mut Plic) };
    let hart = r_tp() as usize;
    plic.enable[hart].write((1 << UART0_IRQ) | (1 << VIRTIO0_IRQ));
    plic.threshold[hart].write(0);
}

pub fn plic_claim() -> u32 {
    let plic = unsafe { &*(PLIC_BASE as *const Plic) };
    let hart = r_tp() as usize;
    plic.claim[hart].read()
}

pub fn plic_complete(irq: u32) {
    let plic = unsafe { &mut *(PLIC_BASE as *mut Plic) };
    let hart = r_tp() as usize;
    plic.claim[hart].write(irq);
}

pub fn devintr() -> u32 {
    let scause = r_scause();
    if scause == 0x8000000000000009 { // Supervisor external interrupt
        let irq = plic_claim();
        match irq {
            UART0_IRQ => crate::drivers::uart::uart_intr(),
            VIRTIO0_IRQ => crate::drivers::virtio::virtio_intr(),
            _ if irq != 0 => crate::arch::console::printk!("unexpected interrupt irq={}\n", irq),
            _ => {}
        }
        if irq != 0 { plic_complete(irq); }
        1
    } else if scause == 0x8000000000000005 { // Timer interrupt
        clock_intr();
        2
    } else {
        0
    }
}

fn clock_intr() {
    if r_tp() == 0 {
        crate::proc::tick();
    }
    w_stimecmp(r_time() + 1000000);
}

pub fn uart_init() {
    // ... UART init
}

pub fn uart_intr() {
    crate::drivers::uart::uart_intr();
}
```

- [ ] **Step 2.5: Create kernel/src/arch/init.rs (kernel entry)**

```rust
// kernel/src/arch/init.rs
use super::asm::*;
use super::paging::{kvm_init, kvm_init_hart};
use super::trap::*;
use super::interrupt::*;
use crate::mm::frame_allocator::kinit;
use crate::proc::procinit;
use crate::fs::fsinit;
use crate::drivers::console::consoleinit;

pub fn init() -> ! {
    let hart_id = r_tp();
    if hart_id == 0 {
        consoleinit();
        crate::arch::console::printk!("\nxv6-rust kernel is booting\n\n");
        kinit();
        let root = kvm_init();
        kvm_init_hart(root);
        procinit();
        trapinit();
        plic_init();
        plic_init_hart();
        crate::drivers::virtio::virtio_init();
        crate::fs::iinit();
        crate::fs::fileinit();
        crate::proc::userinit();
        
        // Signal other harts
        // ... atomic store
    } else {
        // Wait for hart 0
        while !crate::proc::started() {
            core::hint::spin_loop();
        }
        kvm_init_hart(crate::mm::paging::kernel_pagetable());
        trapinit();
        plic_init_hart();
    }
    
    crate::proc::scheduler();
}
```

- [ ] **Step 2.6: Add missing constants to asm.rs**

```rust
// Add to kernel/src/arch/asm.rs
pub const TRAMPOLINE: usize = usize::MAX - 4096 + 1; // 0xFFFFFFFFFFFFF000
pub const KERNBASE: usize = 0x80000000;
pub const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;
pub const UART0: usize = 0x10000000;
pub const VIRTIO0: usize = 0x10001000;
pub const PLIC: usize = 0x0C000000;
pub const MAKE_SATP: fn(usize) -> usize = |ppn| (8 << 60) | (ppn << 12); // Sv39 mode

pub const SSTATUS_SPP: usize = 1 << 8;
pub const SSTATUS_SPIE: usize = 1 << 5;
pub const SSTATUS_SIE: usize = 1 << 1;
```

- [ ] **Step 2.7: Create assembly file for context switch**

```asm
/* kernel/src/arch/asm.S */
.section .text
.global swtch
.global uservec
.global userret
.global kernelvec

/* void swtch(struct context *old, struct context *new) */
swtch:
    sd ra, 0(a0)
    sd sp, 8(a0)
    sd s0, 16(a0)
    sd s1, 24(a0)
    sd s2, 32(a0)
    sd s3, 40(a0)
    sd s4, 48(a0)
    sd s5, 56(a0)
    sd s6, 64(a0)
    sd s7, 72(a0)
    sd s8, 80(a0)
    sd s9, 88(a0)
    sd s10, 96(a0)
    sd s11, 104(a0)
    ld ra, 0(a1)
    ld sp, 8(a1)
    ld s0, 16(a1)
    ld s1, 24(a1)
    ld s2, 32(a1)
    ld s3, 40(a1)
    ld s4, 48(a1)
    ld s5, 56(a1)
    ld s6, 64(a1)
    ld s7, 72(a1)
    ld s8, 80(a1)
    ld s9, 88(a1)
    ld s10, 96(a1)
    ld s11, 104(a1)
    ret

/* uservec: trampoline entry from user mode */
/* Saves user registers to trapframe, sets up kernel registers, jumps to usertrap */
uservec:
    /* swap user/kernel page tables */
    csrrw t0, satp, a0
    sfence.vma
    
    /* save user registers to trapframe (a0 = trapframe pointer) */
    sd ra, 40(a0)
    sd sp, 48(a0)
    sd gp, 56(a0)
    sd tp, 64(a0)
    sd t0, 72(a0)
    sd t1, 80(a0)
    sd t2, 88(a0)
    sd s0, 96(a0)
    sd s1, 104(a0)
    sd a0, 112(a0)
    sd a1, 120(a0)
    sd a2, 128(a0)
    sd a3, 136(a0)
    sd a4, 144(a0)
    sd a5, 152(a0)
    sd a6, 160(a0)
    sd a7, 168(a0)
    sd s2, 176(a0)
    sd s3, 184(a0)
    sd s4, 192(a0)
    sd s5, 200(a0)
    sd s6, 208(a0)
    sd s7, 216(a0)
    sd s8, 224(a0)
    sd s9, 232(a0)
    sd s10, 240(a0)
    sd s11, 248(a0)
    sd t3, 256(a0)
    sd t4, 264(a0)
    sd t5, 272(a0)
    sd t6, 280(a0)
    
    /* load kernel registers from trapframe */
    ld sp, 8(a0)      /* kernel_sp */
    ld tp, 24(a0)     /* kernel_hartid */
    ld t0, 16(a0)     /* kernel_trap */
    
    /* jump to usertrap */
    jr t0

/* userret: return to user mode */
userret:
    /* set up sstatus for user mode */
    csrr t0, sstatus
    li t1, ~0x100     /* clear SPP (bit 8) */
    and t0, t0, t1
    li t1, 0x20       /* set SPIE (bit 5) */
    or t0, t0, t1
    csrw sstatus, t0
    
    /* set sepc from trapframe */
    ld t0, 24(a0)     /* epc */
    csrw sepc, t0
    
    /* restore user registers */
    ld ra, 40(a0)
    ld sp, 48(a0)
    ld gp, 56(a0)
    ld tp, 64(a0)
    ld t0, 72(a0)
    ld t1, 80(a0)
    ld t2, 88(a0)
    ld s0, 96(a0)
    ld s1, 104(a0)
    ld a0, 112(a0)
    ld a1, 120(a0)
    ld a2, 128(a0)
    ld a3, 136(a0)
    ld a4, 144(a0)
    ld a5, 152(a0)
    ld a6, 160(a0)
    ld a7, 168(a0)
    ld s2, 176(a0)
    ld s3, 184(a0)
    ld s4, 192(a0)
    ld s5, 200(a0)
    ld s6, 208(a0)
    ld s7, 216(a0)
    ld s8, 224(a0)
    ld s9, 232(a0)
    ld s10, 240(a0)
    ld s11, 248(a0)
    ld t3, 256(a0)
    ld t4, 264(a0)
    ld t5, 272(a0)
    ld t6, 280(a0)
    
    /* switch to user page table */
    ld t0, 0(a0)      /* kernel_satp */
    csrrw t1, satp, t0
    sfence.vma
    
    sret

/* kernelvec: trap entry from kernel mode */
kernelvec:
    /* save kernel registers, call kerneltrap, restore */
    csrr t0, sstatus
    csrr t1, sepc
    csrr t2, scause
    /* ... save registers to current process's trapframe ... */
    call kerneltrap
    /* ... restore ... */
    sret
```

- [ ] **Step 2.8: Test build and boot**

Run: `cargo build --target riscv64imac-unknown-none-elf -p xv6-kernel`
Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Boots to scheduler, prints "xv6-rust kernel is booting"

- [ ] **Step 2.9: Commit**

```bash
git add kernel/src/arch/
git commit -m "feat(arch): RISC-V CSR, paging, trap, interrupt primitives"
```

---

### Task 3: Memory Management (Frame Allocator, Page Table, Heap)

**Files:**
- Create: `kernel/src/mm/frame_allocator.rs`
- Create: `kernel/src/mm/page_table.rs`
- Create: `kernel/src/mm/address.rs`
- Create: `kernel/src/mm/heap.rs`
- Create: `kernel/src/mm/page_fault.rs`

**Interfaces:**
- Consumes: arch paging, asm primitives
- Produces: `kalloc`/`kfree`, `PageTable` RAII, global allocator, page fault handler

- [ ] **Step 3.1: Create kernel/src/mm/address.rs**

```rust
// kernel/src/mm/address.rs
use core::ops::{Add, Sub, AddAssign, SubAssign};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysAddr(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtAddr(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PhysPageNum(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct VirtPageNum(pub usize);

impl PhysAddr {
    pub const fn new(addr: usize) -> Self { Self(addr) }
    pub fn floor(&self) -> PhysPageNum { PhysPageNum(self.0 / 4096) }
    pub fn ceil(&self) -> PhysPageNum { PhysPageNum((self.0 + 4095) / 4096) }
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl VirtAddr {
    pub const fn new(addr: usize) -> Self { Self(addr) }
    pub fn floor(&self) -> VirtPageNum { VirtPageNum(self.0 / 4096) }
    pub fn ceil(&self) -> VirtPageNum { VirtPageNum((self.0 + 4095) / 4096) }
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl PhysPageNum {
    pub const fn new(ppn: usize) -> Self { Self(ppn) }
    pub fn to_paddr(&self) -> PhysAddr { PhysAddr(self.0 * 4096) }
    pub fn to_vaddr(&self) -> VirtAddr { VirtAddr(self.0 * 4096) }
}

impl VirtPageNum {
    pub const fn new(vpn: usize) -> Self { Self(vpn) }
    pub fn to_vaddr(&self) -> VirtAddr { VirtAddr(self.0 * 4096) }
}

macro_rules! impl_arith {
    ($t:ty) => {
        impl Add<usize> for $t {
            type Output = Self;
            fn add(self, rhs: usize) -> Self { Self(self.0 + rhs) }
        }
        impl Sub<usize> for $t {
            type Output = Self;
            fn sub(self, rhs: usize) -> Self { Self(self.0 - rhs) }
        }
        impl AddAssign<usize> for $t {
            fn add_assign(&mut self, rhs: usize) { self.0 += rhs; }
        }
        impl SubAssign<usize> for $t {
            fn sub_assign(&mut self, rhs: usize) { self.0 -= rhs; }
        }
    };
}

impl_arith!(PhysAddr);
impl_arith!(VirtAddr);
impl_arith!(PhysPageNum);
impl_arith!(VirtPageNum);

impl Sub for PhysAddr { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for VirtAddr { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for PhysPageNum { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for VirtPageNum { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
```

- [ ] **Step 3.2: Create kernel/src/mm/frame_allocator.rs (kalloc/kfree)**

```rust
// kernel/src/mm/frame_allocator.rs
use super::address::{PhysPageNum, PhysAddr};
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use buddy_system_allocator::LockedHeap;
use core::alloc::Layout;

const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024; // 16MB
static mut HEAP: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::new());

pub struct FrameAllocator {
    start_ppn: PhysPageNum,
    end_ppn: PhysPageNum,
    free_list: &'static mut [PhysPageNum],
    free_count: usize,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self { start_ppn: PhysPageNum::new(0), end_ppn: PhysPageNum::new(0), free_list: &mut [], free_count: 0 }
    }
    
    pub fn init(&mut self, start: PhysAddr, end: PhysAddr) {
        self.start_ppn = start.ceil();
        self.end_ppn = end.floor();
        let total = self.end_ppn.0 - self.start_ppn.0;
        // Use first page for free list array
        let list_ptr = self.start_ppn.to_paddr().0 as *mut PhysPageNum;
        self.free_list = unsafe { core::slice::from_raw_parts_mut(list_ptr, total) };
        self.free_count = 0;
        for i in self.start_ppn.0..self.end_ppn.0 {
            self.free_list[self.free_count] = PhysPageNum::new(i);
            self.free_count += 1;
        }
        // Initialize global allocator
        unsafe {
            ALLOCATOR.lock().init(HEAP.as_mut_ptr() as usize, KERNEL_HEAP_SIZE);
        }
    }
    
    pub fn alloc(&mut self) -> Result<PhysPageNum, &'static str> {
        if self.free_count == 0 {
            return Err("out of memory");
        }
        self.free_count -= 1;
        Ok(self.free_list[self.free_count])
    }
    
    pub fn dealloc(&mut self, ppn: PhysPageNum) {
        if self.free_count >= self.free_list.len() {
            panic!("frame allocator free list overflow");
        }
        self.free_list[self.free_count] = ppn;
        self.free_count += 1;
    }
}

pub fn kinit(start: PhysAddr, end: PhysAddr) {
    FRAME_ALLOCATOR.lock().init(start, end);
}

pub fn kalloc() -> Option<PhysPageNum> {
    FRAME_ALLOCATOR.lock().alloc().ok()
}

pub fn kfree(ppn: PhysPageNum) {
    FRAME_ALLOCATOR.lock().dealloc(ppn);
}

pub fn alloc_page() -> Result<PhysPageNum, &'static str> {
    kalloc().ok_or("out of memory")
}

pub fn free_page(ppn: PhysPageNum) {
    kfree(ppn);
}
```

- [ ] **Step 3.3: Create kernel/src/mm/page_table.rs (RAII page table)**

```rust
// kernel/src/mm/page_table.rs
use super::address::{PhysPageNum, VirtAddr, VirtPageNum};
use super::frame_allocator::{alloc_page, free_page};
use crate::arch::paging::{PageTableEntry, PageTableWalker, PAGE_SIZE, PTE_V, PTE_R, PTE_W, PTE_X, PTE_U};
use crate::arch::asm::sfence_vma;
use core::ptr::NonNull;

pub struct PageTable {
    root_ppn: PhysPageNum,
    walker: PageTableWalker,
}

impl PageTable {
    pub fn new() -> Result<Self, &'static str> {
        let root = alloc_page()?;
        // Zero the page
        unsafe { core::ptr::write_bytes(root.to_paddr().0 as *mut u8, 0, PAGE_SIZE) };
        let walker = PageTableWalker::new(root);
        Ok(Self { root_ppn: root, walker })
    }
    
    pub fn from_root(root_ppn: PhysPageNum) -> Self {
        Self { root_ppn, walker: PageTableWalker::new(root_ppn) }
    }
    
    pub fn root_ppn(&self) -> PhysPageNum { self.root_ppn }
    
    pub fn map(&mut self, vaddr: VirtAddr, paddr: PhysAddr, flags: u64) -> Result<(), &'static str> {
        let pte = self.walker.walk(vaddr, true).ok_or("walk failed")?;
        if pte.is_valid() { return Err("remap"); }
        pte.set_ppn(paddr.0 >> 12);
        pte.set_flags(flags | PTE_V);
        Ok(())
    }
    
    pub fn unmap(&mut self, vaddr: VirtAddr) {
        if let Some(pte) = self.walker.walk(vaddr, false) {
            if pte.is_valid() {
                let ppn = pte.ppn();
                pte.0 = 0;
                free_page(ppn);
            }
        }
    }
    
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let pte = self.walker.walk(vaddr, false)?;
        if !pte.is_valid() { return None; }
        let pa = pte.ppn().to_paddr().0 + vaddr.0 % PAGE_SIZE;
        Some(PhysAddr(pa))
    }
    
    pub fn copy_from(&mut self, src: &PageTable, size: usize) -> Result<(), &'static str> {
        // Deep copy of user page table
        for vpn in 0..(size / PAGE_SIZE) {
            let vaddr = VirtAddr(vpn * PAGE_SIZE);
            if let Some(src_pte) = src.walker.walk(vaddr, false) {
                if src_pte.is_valid() {
                    let pa = src_pte.ppn().to_paddr();
                    let flags = src_pte.flags();
                    let new_page = alloc_page()?;
                    unsafe { core::ptr::copy_nonoverlapping(pa.0 as *const u8, new_page.to_paddr().0 as *mut u8, PAGE_SIZE) };
                    self.map(vaddr, new_page.to_paddr(), flags)?;
                }
            }
        }
        Ok(())
    }
    
    pub fn activate(&self) {
        crate::arch::asm::w_satp(crate::arch::asm::MAKE_SATP(self.root_ppn.0));
        sfence_vma();
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        // Recursively free all page table pages
        self.free_walk(self.root_ppn);
    }
}

impl PageTable {
    fn free_walk(&mut self, ppn: PhysPageNum) {
        let pt = unsafe { &mut *(ppn.to_paddr().0 as *mut [PageTableEntry; 512]) };
        for pte in pt {
            if pte.is_valid() && (pte.flags() & (PTE_R | PTE_W | PTE_X)) == 0 {
                // Points to next level page table
                self.free_walk(pte.ppn());
            } else if pte.is_valid() {
                // Leaf mapping - free physical page
                free_page(pte.ppn());
            }
            pte.0 = 0;
        }
        free_page(ppn);
    }
}

pub static KERNEL_PAGETABLE: spin::Once<PageTable> = spin::Once::new();

pub fn kernel_pagetable() -> PhysPageNum {
    KERNEL_PAGETABLE.get().unwrap().root_ppn()
}

pub fn kvminit() {
    let pt = PageTable::new().expect("kvminit: failed to create kernel page table");
    // Map devices, kernel text/data, trampoline, kernel stacks
    map_kernel(&mut pt);
    KERNEL_PAGETABLE.call_once(|| pt);
}

fn map_kernel(pt: &mut PageTable) {
    // UART
    pt.map(VirtAddr(0x10000000), PhysAddr(0x10000000), PTE_R | PTE_W).unwrap();
    // VIRTIO
    pt.map(VirtAddr(0x10001000), PhysAddr(0x10001000), PTE_R | PTE_W).unwrap();
    // PLIC
    pt.map(VirtAddr(0x0C000000), PhysAddr(0x0C000000), PTE_R | PTE_W).unwrap();
    // Kernel text (read-only, executable)
    // ... get etext from linker
    // Kernel data (read-write)
    // Trampoline
    pt.map(VirtAddr(crate::arch::asm::TRAMPOLINE), PhysAddr(crate::arch::asm::TRAMPOLINE), PTE_R | PTE_X).unwrap();
    // Kernel stacks for each CPU
    // ...
}

pub fn kvminithart() {
    KERNEL_PAGETABLE.get().unwrap().activate();
}
```

- [ ] **Step 3.4: Create kernel/src/mm/heap.rs (global allocator)**

```rust
// kernel/src/mm/heap.rs
// Already initialized in frame_allocator.rs via LockedHeap
// This file just re-exports and documents

pub use buddy_system_allocator::LockedHeap;

/// Kernel heap allocator.
/// Initialized in kinit() with 16MB from .bss section.
/// Used for: PageTable, Inode, File, Proc allocations, etc.
pub fn init_heap() {
    // Done in frame_allocator::kinit()
}
```

- [ ] **Step 3.5: Create kernel/src/mm/page_fault.rs (lazy allocation)**

```rust
// kernel/src/mm/page_fault.rs
use super::address::VirtAddr;
use crate::proc::current_process;
use crate::arch::paging::PAGE_SIZE;
use crate::mm::page_table::PageTable;

pub fn handle_page_fault(pagetable: &mut PageTable, vaddr: usize, read: bool) -> Result<(), &'static str> {
    let p = current_process();
    let vaddr = VirtAddr::new(vaddr);
    
    if vaddr.0 >= p.sz {
        return Err("invalid address");
    }
    
    let vpn = vaddr.floor();
    if pagetable.translate(vaddr).is_some() {
        return Err("already mapped");
    }
    
    let ppn = crate::mm::frame_allocator::alloc_page().ok_or("out of memory")?;
    let flags = crate::arch::paging::PTE_R | crate::arch::paging::PTE_U | 
                if read { crate::arch::paging::PTE_W } else { 0 };
    pagetable.map(vaddr, ppn.to_paddr(), flags)?;
    
    Ok(())
}
```

- [ ] **Step 3.6: Test memory allocation**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel mm::frame_allocator`
Expected: Allocates and frees pages correctly

- [ ] **Step 3.7: Test page table**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel mm::page_table`
Expected: Map/unmap/translate work, RAII drop frees pages

- [ ] **Step 3.8: Commit**

```bash
git add kernel/src/mm/
git commit -m "feat(mm): frame allocator, page table, heap, page fault handler"
```

---

### Task 4: Synchronization Primitives

**Files:**
- Create: `kernel/src/sync/spinlock.rs`
- Create: `kernel/src/sync/sleeplock.rs`
- Create: `kernel/src/sync/condvar.rs`

**Interfaces:**
- Consumes: arch asm (intr_on/off), frame allocator
- Produces: `SpinLock<T>`, `SleepLock<T>`, `Condvar` with `sleep`/`wakeup`

- [ ] **Step 4.1: Create kernel/src/sync/spinlock.rs**

```rust
// kernel/src/sync/spinlock.rs
use crate::arch::asm::{intr_on, intr_off, intr_get, push_off, pop_off};
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

pub struct SpinLock<T> {
    locked: AtomicBool,
    name: &'static str,
    data: UnsafeCell<T>,
    cpu: usize,   // For debugging: which CPU holds the lock
}

unsafe impl<T> Sync for SpinLock<T> where T: Send {}
unsafe impl<T> Send for SpinLock<T> where T: Send {}

impl<T> SpinLock<T> {
    pub const fn new(data: T, name: &'static str) -> Self {
        Self {
            locked: AtomicBool::new(false),
            name,
            data: UnsafeCell::new(data),
            cpu: 0,
        }
    }
    
    pub fn acquire(&self) -> SpinLockGuard<'_, T> {
        push_off();
        while self.locked.swap(true, Ordering::Acquire) {
            core::hint::spin_loop();
        }
        self.cpu = crate::arch::asm::r_tp();
        SpinLockGuard { lock: self }
    }
    
    pub fn try_acquire(&self) -> Option<SpinLockGuard<'_, T>> {
        push_off();
        if self.locked.swap(true, Ordering::Acquire) {
            pop_off();
            None
        } else {
            self.cpu = crate::arch::asm::r_tp();
            Some(SpinLockGuard { lock: self })
        }
    }
    
    pub fn holding(&self) -> bool {
        self.locked.load(Ordering::Relaxed) && self.cpu == crate::arch::asm::r_tp()
    }
}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.cpu = 0;
        self.lock.locked.store(false, Ordering::Release);
        pop_off();
    }
}

impl<'a, T> Deref for SpinLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

/// Push interrupt disable nesting
pub fn push_off() {
    let intr = intr_get();
    intr_off();
    // Store previous state in per-CPU variable
    let cpu = crate::proc::mycpu();
    cpu.noff += 1;
    if cpu.noff == 1 {
        cpu.intena = intr;
    }
}

/// Pop interrupt disable nesting
pub fn pop_off() {
    let cpu = crate::proc::mycpu();
    if cpu.noff == 0 {
        panic!("pop_off: noff == 0");
    }
    cpu.noff -= 1;
    if cpu.noff == 0 && cpu.intena {
        intr_on();
    }
}
```

- [ ] **Step 4.2: Create kernel/src/sync/sleeplock.rs**

```rust
// kernel/src/sync/sleeplock.rs
use crate::sync::spinlock::SpinLock;
use crate::proc::{sleep, wakeup, myproc, current_process, ProcState};
use core::cell::UnsafeCell;

pub struct SleepLock<T> {
    locked: bool,
    name: &'static str,
    pid: usize,
    data: UnsafeCell<T>,
}

unsafe impl<T> Sync for SleepLock<T> where T: Send {}
unsafe impl<T> Send for SleepLock<T> where T: Send {}

impl<T> SleepLock<T> {
    pub const fn new(data: T, name: &'static str) -> Self {
        Self { locked: false, name, pid: 0, data: UnsafeCell::new(data) }
    }
    
    pub fn acquire(&self) -> SleepLockGuard<'_, T> {
        let p = myproc();
        let mut guard = self.locked.lock(); // Need internal spinlock for the flag
        while self.locked {
            sleep(self as *const _ as usize, &guard);
        }
        self.locked = true;
        self.pid = p.pid;
        drop(guard);
        SleepLockGuard { lock: self }
    }
    
    pub fn release(&self) {
        let mut guard = self.locked.lock();
        self.locked = false;
        self.pid = 0;
        wakeup(self as *const _ as usize);
    }
    
    pub fn holding(&self) -> bool {
        self.locked
    }
}

pub struct SleepLockGuard<'a, T> {
    lock: &'a SleepLock<T>,
}

impl<'a, T> Drop for SleepLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.release();
    }
}

impl<'a, T> Deref for SleepLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SleepLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

// Need internal spinlock for the flag - use a separate one
struct SleepLockInner {
    locked: bool,
    name: &'static str,
    pid: usize,
}
```

- [ ] **Step 4.3: Create kernel/src/sync/condvar.rs (sleep/wakeup)**

```rust
// kernel/src/sync/condvar.rs
use crate::sync::spinlock::SpinLock;
use crate::proc::{Proc, ProcState, myproc, current_process};
use alloc::collections::VecDeque;
use core::cell::UnsafeCell;

pub struct Condvar {
    wait_queue: SpinLock<VecDeque<*mut Proc>>,
    name: &'static str,
}

impl Condvar {
    pub const fn new(name: &'static str) -> Self {
        Self { wait_queue: SpinLock::new(VecDeque::new(), name), name }
    }
    
    pub fn sleep(&self, lock: &SpinLock<impl Sized>) {
        let p = myproc();
        let mut q = self.wait_queue.acquire();
        q.push_back(p as *mut Proc);
        // Release the external lock while sleeping
        drop(lock);
        // Schedule
        crate::proc::sched();
        // Re-acquire external lock after wakeup
        lock.acquire();
    }
    
    pub fn wakeup(&self) {
        let mut q = self.wait_queue.acquire();
        while let Some(p_ptr) = q.pop_front() {
            let p = unsafe { &mut *p_ptr };
            p.state = ProcState::Runnable;
        }
    }
    
    pub fn wakeup_one(&self) {
        let mut q = self.wait_queue.acquire();
        if let Some(p_ptr) = q.pop_front() {
            let p = unsafe { &mut *p_ptr };
            p.state = ProcState::Runnable;
        }
    }
}

/// Global sleep/wakeup using channel pointers (like xv6)
pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = myproc();
    // Add to global wait queue keyed by chan
    // ... implementation using global HashMap<usize, Condvar>
    drop(lock);
    crate::proc::sched();
    lock.acquire();
}

pub fn wakeup(chan: usize) {
    // Look up condvar for chan, call wakeup()
}
```

- [ ] **Step 4.4: Update proc.rs with wait queue**

```rust
// In kernel/src/proc/mod.rs - add wait queue
use alloc::collections::BTreeMap;
use crate::sync::spinlock::SpinLock;

static WAIT_QUEUES: SpinLock<BTreeMap<usize, alloc::vec::Vec<*mut Proc>>> = 
    SpinLock::new(BTreeMap::new());

pub fn sleep(chan: usize, lock: &SpinLock<impl Sized>) {
    let p = myproc();
    let mut queues = WAIT_QUEUES.acquire();
    queues.entry(chan).or_default().push(p as *mut Proc);
    p.state = ProcState::Sleeping;
    p.chan = chan;
    drop(lock);
    sched();
    lock.acquire();
    // Remove from wait queue after wakeup
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        vec.retain(|&ptr| ptr != p as *mut Proc);
    }
}

pub fn wakeup(chan: usize) {
    let mut queues = WAIT_QUEUES.acquire();
    if let Some(vec) = queues.get_mut(&chan) {
        for &p_ptr in vec.drain(..) {
            let p = unsafe { &mut *p_ptr };
            p.state = ProcState::Runnable;
        }
    }
}
```

- [ ] **Step 4.5: Test spinlock**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel sync::spinlock`
Expected: Lock/unlock works, nesting works, holding() correct

- [ ] **Step 4.6: Test sleep/wakeup**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel sync::condvar`
Expected: Sleep blocks, wakeup unblocks, no lost wakeups

- [ ] **Step 4.7: Commit**

```bash
git add kernel/src/sync/
git commit -m "feat(sync): spinlock, sleeplock, sleep/wakeup condvar"
```

---

### Task 5: Device Drivers

**Files:**
- Create: `kernel/src/drivers/uart.rs`
- Create: `kernel/src/drivers/virtio.rs`
- Create: `kernel/src/drivers/console.rs`

**Interfaces:**
- Consumes: arch interrupt, mmio access
- Produces: Console output, disk I/O, interrupt handlers

- [ ] **Step 5.1: Create kernel/src/drivers/uart.rs (16550 UART)**

```rust
// kernel/src/drivers/uart.rs
use crate::arch::asm::{intr_on, intr_off};
use core::fmt::{self, Write};

const UART0: usize = 0x10000000;
const UART_THR: usize = 0;
const UART_RHR: usize = 0;
const UART_IER: usize = 1;
const UART_FCR: usize = 2;
const UART_LCR: usize = 3;
const UART_MCR: usize = 4;
const UART_LSR: usize = 5;
const UART_LSR_RX: u8 = 1;
const UART_LSR_TX: u8 = 32;

pub fn uart_init() {
    unsafe {
        let uart = UART0 as *mut u8;
        // Disable interrupts
        uart.add(UART_IER).write_volatile(0x00);
        // Set DLAB=1 for baud rate
        uart.add(UART_LCR).write_volatile(0x80);
        // 38400 baud: divisor = 3 (for 115200 base)
        uart.add(0).write_volatile(0x03);
        uart.add(1).write_volatile(0x00);
        // 8 bits, no parity, 1 stop bit, DLAB=0
        uart.add(UART_LCR).write_volatile(0x03);
        // Enable FIFO
        uart.add(UART_FCR).write_volatile(0x07);
        // Enable interrupts
        uart.add(UART_IER).write_volatile(0x01);
    }
}

pub fn uart_putc(c: u8) {
    unsafe {
        let uart = UART0 as *mut u8;
        while (uart.add(UART_LSR).read_volatile() & UART_LSR_TX) == 0 {
            core::hint::spin_loop();
        }
        uart.add(UART_THR).write_volatile(c);
    }
}

pub fn uart_getc() -> Option<u8> {
    unsafe {
        let uart = UART0 as *mut u8;
        if (uart.add(UART_LSR).read_volatile() & UART_LSR_RX) != 0 {
            Some(uart.add(UART_RHR).read_volatile())
        } else {
            None
        }
    }
}

pub fn uart_intr() {
    while let Some(c) = uart_getc() {
        crate::drivers::console::console_intr(c);
    }
}

pub struct UartWriter;

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            uart_putc(c);
        }
        Ok(())
    }
}

pub fn printk(args: fmt::Arguments) {
    intr_off();
    UartWriter.write_fmt(args).unwrap();
    intr_on();
}

#[macro_export]
macro_rules! printk {
    ($($arg:tt)*) => {{
        $crate::drivers::uart::printk(format_args!($($arg)*));
    }};
}
```

- [ ] **Step 5.2: Create kernel/src/drivers/virtio.rs (virtio block)**

```rust
// kernel/src/drivers/virtio.rs
use crate::arch::asm::*;
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::mm::address::PhysAddr;
use core::sync::atomic::{AtomicU16, Ordering};

const VIRTIO0: usize = 0x10001000;
const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x00;
const VIRTIO_MMIO_VERSION: usize = 0x04;
const VIRTIO_MMIO_DEVICE_ID: usize = 0x08;
const VIRTIO_MMIO_VENDOR_ID: usize = 0x0C;
const VIRTIO_MMIO_DEVICE_FEATURES: usize = 0x10;
const VIRTIO_MMIO_DRIVER_FEATURES: usize = 0x20;
const VIRTIO_MMIO_GUEST_PAGE_SIZE: usize = 0x28;
const VIRTIO_MMIO_QUEUE_SEL: usize = 0x30;
const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x34;
const VIRTIO_MMIO_QUEUE_NUM: usize = 0x38;
const VIRTIO_MMIO_QUEUE_READY: usize = 0x44;
const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x80;
const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x84;
const VIRTIO_MMIO_QUEUE_AVAIL_LOW: usize = 0x90;
const VIRTIO_MMIO_QUEUE_AVAIL_HIGH: usize = 0x94;
const VIRTIO_MMIO_QUEUE_USED_LOW: usize = 0xA0;
const VIRTIO_MMIO_QUEUE_USED_HIGH: usize = 0xA4;
const VIRTIO_MMIO_STATUS: usize = 0x70;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;

#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; 8],
}

#[repr(C)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; 8],
}

static DESC: [VirtqDesc; 8] = [VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 8];
static AVAIL: VirtqAvail = VirtqAvail { flags: 0, idx: 0, ring: [0; 8] };
static USED: VirtqUsed = VirtqUsed { flags: 0, idx: 0, ring: [VirtqUsedElem { id: 0, len: 0 }; 8] };
static FREE_DESC: AtomicU16 = AtomicU16::new(0);

pub fn virtio_init() {
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Verify device
        let magic = v.add(VIRTIO_MMIO_MAGIC_VALUE / 4).read_volatile();
        let version = v.add(VIRTIO_MMIO_VERSION / 4).read_volatile();
        let device_id = v.add(VIRTIO_MMIO_DEVICE_ID / 4).read_volatile();
        assert_eq!(magic, 0x74726976); // "virt"
        assert_eq!(version, 2);
        assert_eq!(device_id, 2); // block device
        
        // Reset
        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(0);
        
        // Acknowledge
        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE);
        // Driver
        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER);
        // Features
        v.add(VIRTIO_MMIO_DRIVER_FEATURES / 4).write_volatile(0);
        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK);
        
        // Queue setup
        v.add(VIRTIO_MMIO_QUEUE_SEL / 4).write_volatile(0);
        let max = v.add(VIRTIO_MMIO_QUEUE_NUM_MAX / 4).read_volatile();
        assert!(max >= 8);
        v.add(VIRTIO_MMIO_QUEUE_NUM / 4).write_volatile(8);
        
        // Allocate queue pages
        let desc_page = alloc_page().expect("virtio desc");
        let avail_page = alloc_page().expect("virtio avail");
        let used_page = alloc_page().expect("virtio used");
        
        v.add(VIRTIO_MMIO_QUEUE_DESC_LOW / 4).write_volatile((desc_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_DESC_HIGH / 4).write_volatile((desc_page.0 >> 20) as u32);
        v.add(VIRTIO_MMIO_QUEUE_AVAIL_LOW / 4).write_volatile((avail_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_AVAIL_HIGH / 4).write_volatile((avail_page.0 >> 20) as u32);
        v.add(VIRTIO_MMIO_QUEUE_USED_LOW / 4).write_volatile((used_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_USED_HIGH / 4).write_volatile((used_page.0 >> 20) as u32);
        
        v.add(VIRTIO_MMIO_QUEUE_READY / 4).write_volatile(1);
        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);
        
        // Enable interrupt
        crate::arch::interrupt::plic_init_hart();
    }
}

pub fn virtio_rw(buf: &mut crate::fs::buf::Buf, write: bool) {
    // ... submit request to virtqueue, wait for completion
}

pub fn virtio_intr() {
    // ... process completion
}
```

- [ ] **Step 5.3: Create kernel/src/drivers/console.rs**

```rust
// kernel/src/drivers/console.rs
use crate::drivers::uart::{uart_putc, uart_init};
use crate::sync::spinlock::SpinLock;
use core::fmt::{self, Write};

static CONSOLE_LOCK: SpinLock<()> = SpinLock::new((), "console");

pub fn console_init() {
    uart_init();
}

pub fn console_intr(c: u8) {
    // Handle input (echo, buffer, etc.)
    uart_putc(c);
}

pub fn consputc(c: i32) {
    let _guard = CONSOLE_LOCK.acquire();
    if c == '\n' as i32 {
        uart_putc('\r' as u8);
    }
    uart_putc(c as u8);
}

pub struct ConsoleWriter;

impl Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            consputc(c as i32);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! console_printk {
    ($($arg:tt)*) => {{
        let _guard = $crate::drivers::console::CONSOLE_LOCK.acquire();
        $crate::drivers::console::ConsoleWriter.write_fmt(format_args!($($arg)*)).unwrap();
    }};
}
```

- [ ] **Step 5.4: Test console output**

Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Prints "xv6-rust kernel is booting" to QEMU console

- [ ] **Step 5.5: Test disk I/O**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel drivers::virtio`
Expected: Reads/writes blocks correctly

- [ ] **Step 5.6: Commit**

```bash
git add kernel/src/drivers/
git commit -m "feat(drivers): UART, virtio block, console"
```

---

### Task 6: Process Management

**Files:**
- Create: `kernel/src/proc/process.rs`
- Create: `kernel/src/proc/scheduler.rs`
- Create: `kernel/src/proc/trapframe.rs`
- Create: `kernel/src/proc/context.rs`
- Create: `kernel/src/proc/syscall.rs`
- Create: `kernel/src/proc/mod.rs`

**Interfaces:**
- Consumes: sync primitives, mm, trap, arch
- Produces: Process struct, scheduler, syscall dispatch, fork/exec/wait

- [ ] **Step 6.1: Create kernel/src/proc/process.rs**

```rust
// kernel/src/proc/process.rs
use crate::mm::page_table::PageTable;
use crate::arch::trap::TrapFrame;
use crate::arch::asm::Context;
use crate::sync::spinlock::SpinLock;
use crate::fs::{Inode, File};
use alloc::vec::Vec;
use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcState {
    Unused,
    Used,
    Sleeping,
    Runnable,
    Running,
    Zombie,
}

pub struct Proc {
    lock: SpinLock<ProcInner>,
}

struct ProcInner {
    state: ProcState,
    chan: usize,
    killed: bool,
    xstate: i32,
    pid: usize,
    parent: Option<*mut Proc>,
    kstack: usize,
    sz: usize,
    pagetable: Option<PageTable>,
    trapframe: *mut TrapFrame,
    context: Context,
    ofile: [Option<File>; 16],
    cwd: Option<Inode>,
    name: [u8; 16],
}

impl Proc {
    pub const fn new() -> Self {
        Self {
            lock: SpinLock::new(ProcInner {
                state: ProcState::Unused,
                chan: 0,
                killed: false,
                xstate: 0,
                pid: 0,
                parent: None,
                kstack: 0,
                sz: 0,
                pagetable: None,
                trapframe: core::ptr::null_mut(),
                context: Context::new(),
                ofile: [None; 16],
                cwd: None,
                name: [0; 16],
            }, "proc")
        }
    }
    
    pub fn pid(&self) -> usize { self.lock().pid }
    pub fn state(&self) -> ProcState { self.lock().state }
    pub fn is_killed(&self) -> bool { self.lock().killed }
    pub fn kill(&self) { self.lock().killed = true; }
    pub fn set_state(&self, state: ProcState) { self.lock().state = state; }
    // ... more accessors
}
```

- [ ] **Step 6.2: Create kernel/src/proc/scheduler.rs**

```rust
// kernel/src/proc/scheduler.rs
use crate::proc::process::{Proc, ProcState, NPROC};
use crate::arch::trap::Context;
use crate::arch::asm::{intr_on, intr_off, w_satp, MAKE_SATP};

static PROCS: [Proc; NPROC] = [const { Proc::new() }; NPROC];
static mut SCHEDULER_STARTED: bool = false;

pub fn procinit() {
    // Initialize process table
    for (i, p) in PROCS.iter().enumerate() {
        let mut inner = p.lock();
        inner.pid = 0; // Will be set on alloc
    }
}

pub fn alloc_proc() -> Option<&'static Proc> {
    for p in &PROCS {
        let mut inner = p.lock();
        if inner.state == ProcState::Unused {
            inner.state = ProcState::Used;
            inner.pid = next_pid();
            inner.kstack = alloc_kernel_stack();
            inner.trapframe = (inner.kstack + 4096 - core::mem::size_of::<TrapFrame>()) as *mut TrapFrame;
            return Some(p);
        }
    }
    None
}

pub fn free_proc(p: &Proc) {
    let mut inner = p.lock();
    inner.state = ProcState::Unused;
    inner.pid = 0;
    if inner.kstack != 0 {
        free_kernel_stack(inner.kstack);
    }
    if let Some(pt) = inner.pagetable.take() {
        drop(pt);
    }
}

pub fn scheduler() -> ! {
    unsafe { SCHEDULER_STARTED = true; }
    intr_on();
    loop {
        for p in &PROCS {
            let mut inner = p.lock();
            if inner.state == ProcState::Runnable {
                inner.state = ProcState::Running;
                drop(inner);
                
                let p = &PROCS[inner.pid]; // Need proper reference
                let satp = MAKE_SATP(p.lock().pagetable.as_ref().unwrap().root_ppn().0);
                w_satp(satp);
                
                crate::arch::trap::context_switch(
                    &mut cpu_context(),
                    &p.lock().context
                );
                
                w_satp(kernel_pagetable());
            }
        }
    }
}

pub fn yield_now() {
    let p = current_process();
    let mut inner = p.lock();
    inner.state = ProcState::Runnable;
    drop(inner);
    sched();
}

pub fn sched() {
    let p = current_process();
    let mut inner = p.lock();
    if intr_get() {
        panic!("sched: interrupts enabled");
    }
    if inner.state == ProcState::Running {
        panic!("sched: running");
    }
    crate::arch::trap::context_switch(&mut inner.context, &cpu_context());
}
```

- [ ] **Step 6.3: Create kernel/src/proc/syscall.rs**

```rust
// kernel/src/proc/syscall.rs
use crate::arch::trap::TrapFrame;
use crate::proc::current_process;

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
        SYS_WRITE => sys_write(tf.a0, tf.a1, tf.a2) as usize,
        SYS_MKNOD => sys_mknod(tf.a0, tf.a1, tf.a2) as usize,
        SYS_UNLINK => sys_unlink(tf.a0) as usize,
        SYS_LINK => sys_link(tf.a0, tf.a1) as usize,
        SYS_MKDIR => sys_mkdir(tf.a0) as usize,
        SYS_CLOSE => sys_close(tf.a0) as usize,
        _ => {
            crate::arch::console::printk!("unknown syscall {}\n", num);
            -1isize as usize
        }
    };
}

// Syscall implementations...
fn sys_fork() -> isize { ... }
fn sys_exit(code: i32) -> ! { ... }
fn sys_wait(addr: usize) -> isize { ... }
fn sys_pipe(fd0: usize, fd1: usize) -> isize { ... }
fn sys_read(fd: usize, addr: usize, n: usize) -> isize { ... }
fn sys_write(fd: usize, addr: usize, n: usize) -> isize { ... }
fn sys_close(fd: usize) -> isize { ... }
fn sys_kill(pid: usize) -> isize { ... }
fn sys_exec(path: usize, argv: usize) -> isize { ... }
fn sys_fstat(fd: usize, addr: usize) -> isize { ... }
fn sys_chdir(path: usize) -> isize { ... }
fn sys_dup(fd: usize) -> isize { ... }
fn sys_getpid() -> isize { ... }
fn sys_sbrk(n: usize) -> isize { ... }
fn sys_sleep(ticks: usize) -> isize { ... }
fn sys_uptime() -> isize { ... }
fn sys_open(path: usize, flags: usize, mode: usize) -> isize { ... }
fn sys_mknod(path: usize, major: usize, minor: usize) -> isize { ... }
fn sys_unlink(path: usize) -> isize { ... }
fn sys_link(old: usize, new: usize) -> isize { ... }
fn sys_mkdir(path: usize) -> isize { ... }
```

- [ ] **Step 6.4: Create kernel/src/proc/mod.rs (public API)**

```rust
// kernel/src/proc/mod.rs
pub mod process;
pub mod scheduler;
pub mod syscall;
pub mod trapframe;
pub mod context;

use crate::proc::process::Proc;
use crate::arch::asm::r_tp;

static CPU: [Cpu; NCPU] = [const { Cpu::new() }; NCPU];

struct Cpu {
    proc: Option<&'static Proc>,
    context: crate::arch::trap::Context,
    noff: usize,
    intena: bool,
}

impl Cpu {
    const fn new() -> Self { Self { proc: None, context: crate::arch::trap::Context::new(), noff: 0, intena: false } }
}

pub fn mycpu() -> &'static mut Cpu {
    let hart = r_tp();
    unsafe { &mut CPU[hart] }
}

pub fn current_process() -> &'static Proc {
    mycpu().proc.expect("no current process")
}

pub fn current_process_opt() -> Option<&'static Proc> {
    mycpu().proc
}

pub fn started() -> bool {
    unsafe { crate::proc::scheduler::SCHEDULER_STARTED }
}

pub fn tick() {
    // Increment ticks, wakeup sleepers
}

pub fn userinit() {
    // Create init process
}
```

- [ ] **Step 6.5: Test process creation**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel proc`
Expected: alloc_proc/free_proc work, scheduler loops

- [ ] **Step 6.6: Test syscall dispatch**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel proc::syscall`
Expected: Syscall numbers map to correct handlers

- [ ] **Step 6.7: Commit**

```bash
git add kernel/src/proc/
git commit -m "feat(proc): process, scheduler, syscall dispatch"
```

---

### Task 7: File System

**Files:**
- Create: `kernel/src/fs/buf.rs`
- Create: `kernel/src/fs/inode.rs`
- Create: `kernel/src/fs/log.rs`
- Create: `kernel/src/fs/file.rs`
- Create: `kernel/src/fs/mod.rs`

**Interfaces:**
- Consumes: drivers/virtio, mm, sync, proc
- Produces: Buffer cache, inode layer, logging, file descriptors

- [ ] **Step 7.1: Create kernel/src/fs/buf.rs**

```rust
// kernel/src/fs/buf.rs
use crate::mm::frame_allocator::{alloc_page, free_page};
use crate::mm::address::PhysAddr;
use crate::drivers::virtio::virtio_rw;
use crate::sync::spinlock::SpinLock;
use crate::sync::sleeplock::SleepLock;
use core::cell::UnsafeCell;

pub const BSIZE: usize = 1024;

pub struct Buf {
    lock: SleepLock<()>,
    blockno: u32,
    dev: u32,
    refcnt: usize,
    valid: bool,
    data: [u8; BSIZE],
}

impl Buf {
    pub fn new(blockno: u32, dev: u32) -> Self {
        Self {
            lock: SleepLock::new((), "buf"),
            blockno,
            dev,
            refcnt: 0,
            valid: false,
            data: [0; BSIZE],
        }
    }
    
    pub fn lock(&self) -> SleepLockGuard<()> { self.lock.acquire() }
    pub fn data(&self) -> &[u8] { &self.data }
    pub fn data_mut(&mut self) -> &mut [u8] { &mut self.data }
}

static BUF_CACHE: SpinLock<BufCache> = SpinLock::new(BufCache::new(), "bcache");

struct BufCache {
    buffers: [Option<Buf>; NBUF],
    head: Option<BufRef>, // LRU list
}

impl BufCache {
    const fn new() -> Self { Self { buffers: [None; NBUF], head: None } }
}

pub fn binit() {
    // Initialize buffer cache
}

pub fn bread(dev: u32, blockno: u32) -> BufRef {
    // Look up in cache, or allocate new, read from disk
}

pub fn brelse(buf: BufRef) {
    // Decrement refcnt, move to head of LRU if 0
}

pub fn bwrite(buf: BufRef) {
    // Write to disk via virtio
}

pub fn bpin(buf: BufRef) {
    // Increment refcnt (for log)
}

pub fn bunpin(buf: BufRef) {
    // Decrement refcnt
}
```

- [ ] **Step 7.2: Create kernel/src/fs/inode.rs**

```rust
// kernel/src/fs/inode.rs
use crate::sync::spinlock::SpinLock;
use crate::sync::sleeplock::SleepLock;
use crate::fs::buf::{bread, brelse, bwrite, BSIZE};
use alloc::vec::Vec;

pub const NDIRECT: usize = 12;
pub const NINDIRECT: usize = BSIZE / 4;
pub const MAXFILE: usize = NDIRECT + NINDIRECT;

#[repr(C)]
pub struct DiskInode {
    typ: u16,
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

pub enum InodeType {
    None = 0,
    Dir = 1,
    File = 2,
    Device = 3,
}

pub struct Inode {
    lock: SleepLock<()>,
    spinlock: SpinLock<InodeInner>,
    dev: u32,
    inum: u32,
    refcnt: usize,
}

struct InodeInner {
    typ: InodeType,
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

impl Inode {
    pub fn new(dev: u32, inum: u32) -> Self { ... }
    pub fn lock(&self) -> SleepLockGuard<()> { self.lock.acquire() }
    pub fn read(&self, dst: &mut [u8], off: usize, n: usize) -> usize { ... }
    pub fn write(&self, src: &[u8], off: usize, n: usize) -> usize { ... }
    pub fn truncate(&self) { ... }
}

pub fn ialloc(dev: u32, typ: InodeType) -> Option<Inode> { ... }
pub fn iget(dev: u32, inum: u32) -> Inode { ... }
pub fn iput(inode: &Inode) { ... }
```

- [ ] **Step 7.3: Create kernel/src/fs/log.rs**

```rust
// kernel/src/fs/log.rs
use crate::fs::buf::{bread, bwrite, brelse, bpin, bunpin, BSIZE};
use crate::sync::spinlock::SpinLock;
use crate::sync::condvar::Condvar;
use alloc::vec::Vec;

const LOGSIZE: usize = 30;
const LOG_MAGIC: u32 = 0x584C4F47; // "XLOG"

struct Log {
    lock: SpinLock<LogInner>,
    committed: Condvar,
}

struct LogInner {
    dev: u32,
    start: u32,
    size: u32,
    outstanding: usize,
    committing: bool,
    lh: LogHeader,
}

#[repr(C)]
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

static LOG: Log = Log::new();

impl Log {
    const fn new() -> Self {
        Self {
            lock: SpinLock::new(LogInner {
                dev: 0, start: 0, size: 0,
                outstanding: 0, committing: false,
                lh: LogHeader { n: 0, block: [0; LOGSIZE] },
            }, "log"),
            committed: Condvar::new("log"),
        }
    }
}

pub fn initlog(dev: u32, sb: &SuperBlock) {
    let mut log = LOG.lock();
    log.dev = dev;
    log.start = sb.logstart;
    log.size = sb.nlog;
}

pub fn begin_op() {
    let mut log = LOG.lock();
    log.outstanding += 1;
}

pub fn end_op() {
    let mut log = LOG.lock();
    log.outstanding -= 1;
    if log.outstanding == 0 && log.committing {
        log.committed.wakeup_all();
    }
}

fn write_log() { ... }
fn install_trans() { ... }
fn recover_from_log() { ... }
```

- [ ] **Step 7.4: Create kernel/src/fs/file.rs**

```rust
// kernel/src/fs/file.rs
use crate::fs::inode::Inode;
use crate::fs::pipe::Pipe;
use crate::sync::spinlock::SpinLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    None,
    Pipe,
    Inode,
    Device,
}

pub struct File {
    lock: SpinLock<FileInner>,
}

struct FileInner {
    typ: FileType,
    refcnt: usize,
    readable: bool,
    writable: bool,
    pipe: Option<Pipe>,
    inode: Option<Inode>,
    off: usize,
    major: u16,
}

impl File {
    pub fn alloc() -> Option<File> { ... }
    pub fn dup(&self) -> File { ... }
    pub fn close(&self) { ... }
    pub fn read(&self, addr: usize, n: usize) -> isize { ... }
    pub fn write(&self, addr: usize, n: usize) -> isize { ... }
    pub fn stat(&self, addr: usize) -> isize { ... }
}

pub fn fileinit() { ... }
```

- [ ] **Step 7.5: Test file system**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel fs`
Expected: Buffer cache, inode ops, logging work

- [ ] **Step 7.6: Commit**

```bash
git add kernel/src/fs/
git commit -m "feat(fs): buffer cache, inode, logging, file layer"
```

---

### Task 8: User Space Programs

**Files:**
- Create: `user-lib/src/syscall.rs` (full implementation)
- Create: `user-lib/src/stdio.rs`
- Create: `user-lib/src/string.rs`
- Create: `user-lib/src/fs.rs`
- Create: `user-lib/src/process.rs`
- Create: `user/src/bin/sh.rs`
- Create: `user/src/bin/ls.rs`
- Create: `user/src/bin/cat.rs`
- ... all other user programs

**Interfaces:**
- Consumes: syscall ABI
- Produces: Working user programs

- [ ] **Step 8.1: Implement full syscall wrappers in user-lib**

```rust
// user-lib/src/syscall.rs
#![allow(dead_code)]

pub const SYS_FORK: usize = 1;
pub const SYS_EXIT: usize = 2;
// ... all syscall numbers

#[inline(always)]
fn syscall0(n: usize) -> usize {
    let ret: usize;
    unsafe { core::arch::asm!("ecall", in("a7") n, lateout("a0") ret, options(nostack)) };
    ret
}

#[inline(always)]
fn syscall1(n: usize, a0: usize) -> usize {
    let ret: usize;
    unsafe { core::arch::asm!("ecall", in("a7") n, in("a0") a0, lateout("a0") ret, options(nostack)) };
    ret
}

// ... syscall2 through syscall6

pub fn fork() -> isize { syscall0(SYS_FORK) as isize }
pub fn exit(code: i32) -> ! { syscall1(SYS_EXIT, code as usize); loop {} }
pub fn wait(addr: *mut i32) -> isize { syscall1(SYS_WAIT, addr as usize) as isize }
pub fn pipe(fds: &mut [i32; 2]) -> isize { syscall1(SYS_PIPE, fds.as_mut_ptr() as usize) as isize }
pub fn read(fd: i32, buf: &mut [u8]) -> isize { syscall3(SYS_READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) as isize }
pub fn write(fd: i32, buf: &[u8]) -> isize { syscall3(SYS_WRITE, fd as usize, buf.as_ptr() as usize, buf.len()) as isize }
pub fn close(fd: i32) -> isize { syscall1(SYS_CLOSE, fd as usize) as isize }
pub fn kill(pid: i32) -> isize { syscall1(SYS_KILL, pid as usize) as isize }
pub fn exec(path: &str, argv: &[*const u8]) -> isize { ... }
pub fn fstat(fd: i32, st: &mut Stat) -> isize { ... }
pub fn chdir(path: &str) -> isize { ... }
pub fn dup(fd: i32) -> isize { syscall1(SYS_DUP, fd as usize) as isize }
pub fn getpid() -> isize { syscall0(SYS_GETPID) as isize }
pub fn sbrk(n: isize) -> *mut u8 { syscall1(SYS_SBRK, n as usize) as *mut u8 }
pub fn sleep(ticks: usize) -> isize { syscall1(SYS_SLEEP, ticks) as isize }
pub fn uptime() -> isize { syscall0(SYS_UPTIME) as isize }
pub fn open(path: &str, flags: i32) -> isize { ... }
pub fn mknod(path: &str, major: i32, minor: i32) -> isize { ... }
pub fn unlink(path: &str) -> isize { ... }
pub fn link(old: &str, new: &str) -> isize { ... }
pub fn mkdir(path: &str) -> isize { ... }
```

- [ ] **Step 8.2: Implement printf, stdio**

```rust
// user-lib/src/stdio.rs
use core::fmt::{self, Write};

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::syscall::write(1, s.as_bytes());
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! println {
    () => { print(format_args!("\n")) };
    ($($arg:tt)*) => { print(format_args!("{}\n", format_args!($($arg)*))) };
}
```

- [ ] **Step 8.3: Implement shell (sh.rs)**

```rust
// user/src/bin/sh.rs
#![no_std]
#![no_main]

use xv6_user_lib::{print, println, syscall, string, fs, process};
use alloc::string::String;
use alloc::vec::Vec;

#[no_mangle]
fn main() -> isize {
    println!("xv6-rust shell");
    loop {
        print!("$ ");
        let mut input = String::new();
        // Read line from stdin
        // Parse command
        // Execute built-in or external
    }
}
```

- [ ] **Step 8.4: Build user programs**

Run: `cargo build --target riscv64gc-unknown-linux-gnu -p xv6-user`
Expected: All binaries built in `target/riscv64gc-unknown-linux-gnu/debug/`

- [ ] **Step 8.5: Create initramfs / fs.img with user programs**

```bash
# Build script to create fs.img with all user binaries
```

- [ ] **Step 8.6: Test in QEMU**

Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Boots to shell prompt, can run commands

- [ ] **Step 8.7: Commit**

```bash
git add user/ user-lib/
git commit -m "feat(user): user library and shell"
```

---

### Task 9: Integration Testing & usertests

**Files:**
- Create: `tests/integration.rs`
- Create: `run_usertests.sh`

**Interfaces:**
- Consumes: All kernel + user code
- Produces: Passing test suite

- [ ] **Step 9.1: Create integration test harness**

```rust
// tests/integration.rs
#![no_std]
#![no_main]

use xv6_kernel::{arch, proc, fs, drivers};

#[test_case]
fn test_boot() {
    // Kernel boots to scheduler
}

#[test_case]
fn test_fork() {
    // fork() creates child process
}

#[test_case]
fn test_exec() {
    // exec() replaces process image
}

#[test_case]
fn test_pipe() {
    // pipe() creates readable/writable fds
}

#[test_case]
fn test_file_ops() {
    // open/read/write/close work
}

#[test_case]
fn test_forktest() {
    // forktest passes (stress test)
}
```

- [ ] **Step 9.2: Run usertests**

```bash
#!/bin/bash
# run_usertests.sh
cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel -- usertests
```

Expected: All usertests pass (forktest, grind, stressfs, etc.)

- [ ] **Step 9.3: Performance benchmark**

Compare boot time, syscall latency, context switch vs C xv6

- [ ] **Step 9.4: Commit**

```bash
git add tests/ run_usertests.sh
git commit -m "test: integration tests, usertests pass"
```

---

### Task 10: Documentation & Polish

**Files:**
- Create: `README.md` (updated)
- Create: `docs/architecture.md`
- Create: `docs/migration.md`

- [ ] **Step 10.1: Write architecture documentation**

- [ ] **Step 10.2: Write migration guide from C xv6**

- [ ] **Step 10.3: Add inline docs to all public APIs**

- [ ] **Step 10.4: Final commit**

```bash
git add docs/ README.md
git commit -m "docs: architecture, migration guide, API docs"
```

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-07-22-xv6-rust-rewrite-plan.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**