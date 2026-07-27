// kernel/src/arch/asm.rs
use core::arch::asm;

macro_rules! read_csr {
    ($csr:ident) => {{
        let val: usize;
        unsafe { asm!(concat!("csrr {}, ", stringify!($csr)), out(reg) val) };
        val
    }};
}

macro_rules! write_csr {
    ($csr:ident, $val:expr) => {{
        unsafe { asm!(concat!("csrw ", stringify!($csr), ", {}"), in(reg) $val) };
    }};
}

macro_rules! swap_csr {
    ($csr:ident, $val:expr) => {{
        let old: usize;
        unsafe { asm!(concat!("csrrw {}, ", stringify!($csr), ", {}"), out(reg) old, in(reg) $val) };
        old
    }};
}

macro_rules! set_csr {
    ($csr:ident, $val:expr) => {{
        unsafe { asm!(concat!("csrs ", stringify!($csr), ", {}"), in(reg) $val) };
    }};
}

macro_rules! clear_csr {
    ($csr:ident, $val:expr) => {{
        unsafe { asm!(concat!("csrc ", stringify!($csr), ", {}"), in(reg) $val) };
    }};
}

/// Read SSTATUS
#[inline]
pub fn r_sstatus() -> usize {
    read_csr!(sstatus)
}

/// Write SSTATUS
#[inline]
pub fn w_sstatus(val: usize) {
    write_csr!(sstatus, val);
}

/// Read SEPC
#[inline]
pub fn r_sepc() -> usize {
    read_csr!(sepc)
}

/// Write SEPC
#[inline]
pub fn w_sepc(val: usize) {
    write_csr!(sepc, val);
}

/// Read SCAUSE
#[inline]
pub fn r_scause() -> usize {
    read_csr!(scause)
}

/// Read STVAL
#[inline]
pub fn r_stval() -> usize {
    read_csr!(stval)
}

/// Read SATP
#[inline]
pub fn r_satp() -> usize {
    read_csr!(satp)
}

/// Write SATP
#[inline]
pub fn w_satp(val: usize) {
    write_csr!(satp, val);
}

/// Write SCRATCH
#[inline]
pub fn w_sscratch(val: usize) {
    write_csr!(sscratch, val);
}

/// Read SCRATCH
#[inline]
pub fn r_sscratch() -> usize {
    read_csr!(sscratch)
}

/// Read STVEC
#[inline]
pub fn r_stvec() -> usize {
    read_csr!(stvec)
}

/// Write STVEC
#[inline]
pub fn w_stvec(val: usize) {
    write_csr!(stvec, val);
}

/// Read SIE
#[inline]
pub fn r_sie() -> usize {
    read_csr!(sie)
}

/// Write SIE
#[inline]
pub fn w_sie(val: usize) {
    write_csr!(sie, val);
}

/// Read SIP
#[inline]
pub fn r_sip() -> usize {
    read_csr!(sip)
}

/// Read the `tp` register, which holds this hart's id (set from `mhartid` in
/// `mstart`). `tp` (x4) is a general-purpose register, NOT a CSR — the old code
/// read CSR 0x106 (scounteren), which is always 0, so every hart looked like
/// hart 0. That was invisible on a single hart but breaks all per-hart state
/// (mycpu, spinlock ownership, PLIC contexts) under SMP.
#[inline]
pub fn r_tp() -> usize {
    let val: usize;
    unsafe { asm!("mv {}, tp", out(reg) val) };
    val
}

/// Read TIME
#[inline]
pub fn r_time() -> usize {
    read_csr!(time)
}

/// Write STIMECMP
#[inline]
pub fn w_stimecmp(val: usize) {
    write_csr!(stimecmp, val);
}

/// Enable interrupts
#[inline]
pub fn intr_on() {
    set_csr!(sstatus, 1 << 1); // SIE bit
}

/// Disable interrupts
#[inline]
pub fn intr_off() {
    clear_csr!(sstatus, 1 << 1);
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

pub const TRAMPOLINE: usize = usize::MAX - 4096 + 1; // 0xFFFFFFFFFFFFF000
/// Per-process trapframe, mapped one page below the trampoline in every user
/// page table. Must match the `.equ TRAPFRAME` constant in arch/asm.S.
pub const TRAPFRAME: usize = TRAMPOLINE - 4096; // 0xFFFFFFFFFFFFE000
pub const KERNBASE: usize = 0x80000000;
pub const PHYSTOP: usize = KERNBASE + 128 * 1024 * 1024;
pub const UART0: usize = 0x10000000;
pub const VIRTIO0: usize = 0x10001000;
pub const PLIC: usize = 0x0C000000;

pub const fn make_satp(ppn: usize) -> usize {
    // Sv39: MODE(8) in bits [63:60], PPN in bits [43:0]. `ppn` is already a
    // physical page number (physaddr >> 12), so it goes in directly.
    (8 << 60) | ppn
}

pub const SSTATUS_SPP: usize = 1 << 8;
pub const SSTATUS_SPIE: usize = 1 << 5;
pub const SSTATUS_SIE: usize = 1 << 1;