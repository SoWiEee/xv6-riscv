// kernel/src/arch/init.rs
use super::asm::*;
use super::interrupt::*;
use crate::mm::frame_allocator::kinit;
use crate::proc::scheduler::procinit;
use crate::arch::console::consoleinit;
use crate::mm::page_table::{kvminit, kvminithart};
use crate::mm::address::PhysAddr;
use crate::proc::scheduler::scheduler;
use core::sync::atomic::{AtomicBool, Ordering};

static STARTED: AtomicBool = AtomicBool::new(false);

/// Machine-mode entry, reached from `_start` in entry.S. Performs the one-time
/// M-mode configuration and `mret`s into `init()` running in Supervisor mode.
///
/// Without this the kernel would keep running in M-mode, where `satp` is ignored
/// and paging never takes effect (so the trampoline / user page tables and user
/// mode itself cannot work).
#[unsafe(no_mangle)]
pub extern "C" fn mstart() -> ! {
    use core::arch::asm;
    unsafe {
        // mret should land us in Supervisor mode: mstatus.MPP = 0b01 (S).
        let mut mstatus: usize;
        asm!("csrr {}, mstatus", out(reg) mstatus);
        mstatus &= !(0b11usize << 11);
        mstatus |= 0b01usize << 11;
        asm!("csrw mstatus, {}", in(reg) mstatus);

        // mret target: init() in S-mode.
        asm!("csrw mepc, {}", in(reg) init as usize);

        // Leave paging disabled for now; kvminithart() enables it in S-mode.
        asm!("csrw satp, x0");

        // Delegate all exceptions and interrupts to S-mode.
        asm!("csrw medeleg, {}", in(reg) 0xffffusize);
        asm!("csrw mideleg, {}", in(reg) 0xffffusize);

        // Enable the S-mode interrupt sources (SEIE | STIE | SSIE).
        let mut sie: usize;
        asm!("csrr {}, sie", out(reg) sie);
        sie |= (1usize << 9) | (1usize << 5) | (1usize << 1);
        asm!("csrw sie, {}", in(reg) sie);

        // Give S-mode access to all of physical memory (PMP entry 0, TOR,
        // R/W/X over [0, 0x3fffffffffffff << 2)).
        asm!("csrw pmpaddr0, {}", in(reg) 0x3fffffffffffffusize);
        asm!("csrw pmpcfg0, {}", in(reg) 0xfusize);

        // Enable the Sstc extension (menvcfg.STCE) so S-mode can program
        // stimecmp directly, and let S-mode read the time counter.
        asm!("csrw menvcfg, {}", in(reg) 1usize << 63);
        asm!("csrw mcounteren, {}", in(reg) 0b111usize);

        // Keep the hart id in tp for r_tp() in S-mode.
        asm!("csrr tp, mhartid");

        // Enter S-mode at init(). Does not return.
        asm!("mret", options(noreturn));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn init() -> ! {
    let hart_id = r_tp();
    
    // All harts wait here until hart 0 signals
    if hart_id != 0 {
        while !started() {
            core::hint::spin_loop();
        }
    }
    
    if hart_id == 0 {
        // Hart 0 does full initialization
        consoleinit();
        crate::arch::console::printk(format_args!("\nxv6-rust kernel is booting\n\n"));
        // Get physical memory range from linker script. Start the free pool at
        // _kernel_end (past the per-hart boot stacks) — starting at `end` would
        // put the boot stacks inside the allocatable pool and hand them out.
        unsafe extern "C" {
            fn _kernel_end();
        }
        let start = _kernel_end as usize;
        let end_addr = crate::arch::asm::PHYSTOP;

        // Initialize frame allocator first so kvminit can allocate pages
        kinit(
            PhysAddr::new(start),
            PhysAddr::new(end_addr),
        );

        // Initialize kernel page table
        kvminit();
        kvminithart();

        procinit();
        trapinit();
        plic_init();
        crate::drivers::virtio::virtio_init();
        // Full FS init: buffer cache, inode cache, log setup + crash recovery.
        // Runs single-threaded here (before the scheduler), so the buffer
        // sleeplocks spin rather than sleep. Must follow virtio_init since log
        // recovery reads the superblock and replays the on-disk log.
        crate::fs::fsinit();
        crate::fs::fileinit();
        crate::proc::userinit();

        // Signal other harts
        STARTED.store(true, Ordering::SeqCst);
    }
    
    // All non-zero harts initialize their page tables and traps
    if hart_id != 0 {
        kvminithart();
        trapinit();
        plic_init_hart();
    }
    
    scheduler();
}

fn started() -> bool {
    STARTED.load(Ordering::SeqCst)
}

pub fn trapinit() {
    // Set up trap vector for kernel mode
    w_stvec(super::trap::kernelvec_addr());
    // Set initial timer interrupt
    w_stimecmp(r_time() + 1000000);
}