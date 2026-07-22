// kernel/src/arch/asm.rs
use core::arch::asm;

macro_rules! read_csr {
    ($csr:expr) => {{
        let val: usize;
        unsafe { asm!(concat!("csrr {}, ", stringify!($csr)), out(reg) val) };
        val
    }};
}

macro_rules! write_csr {
    ($csr:expr, $val:expr) => {{
        unsafe { asm!(concat!("csrw ", stringify!($csr), ", {}"), in(reg) $val) };
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
    unsafe { asm!("csrs sstatus, {}", in(reg) (1 << 1)) }; // SIE bit
}

/// Disable interrupts
#[inline]
pub fn intr_off() {
    unsafe { asm!("csrc sstatus, {}", in(reg) (1 << 1)) };
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