// kernel/src/arch/registers.rs
use tock_registers::register_bitfields;

register_bitfields! {u64,
    SSTATUS [
        SPP OFFSET(8) NUMBITS(1) [],
        SPIE OFFSET(5) NUMBITS(1) [],
        SIE OFFSET(1) NUMBITS(1) [],
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
}

// PLIC register offsets
pub const PLIC_PRIORITY_BASE: usize = 0x0000;
pub const PLIC_PENDING_BASE: usize = 0x2000;
pub const PLIC_ENABLE_BASE: usize = 0x4000;
pub const PLIC_THRESHOLD_BASE: usize = 0x3c000;
pub const PLIC_CLAIM_BASE: usize = 0x74000;
pub const PLIC_HART_STRIDE: usize = 0x10000;

pub const PTE_V: u64 = 1 << 0;
pub const PTE_R: u64 = 1 << 1;
pub const PTE_W: u64 = 1 << 2;
pub const PTE_X: u64 = 1 << 3;
pub const PTE_U: u64 = 1 << 4;
pub const PTE_G: u64 = 1 << 5;
pub const PTE_A: u64 = 1 << 6;
pub const PTE_D: u64 = 1 << 7;

pub const PGSIZE: usize = 4096;
// Uppercase names deliberately mirror C xv6's PGROUNDUP/PGROUNDDOWN macros.
#[allow(non_snake_case)]
pub fn PGROUNDUP(x: usize) -> usize { (x + PGSIZE - 1) & !(PGSIZE - 1) }
#[allow(non_snake_case)]
pub fn PGROUNDDOWN(x: usize) -> usize { x & !(PGSIZE - 1) }