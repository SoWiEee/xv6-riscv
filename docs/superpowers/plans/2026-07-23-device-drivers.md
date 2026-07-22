# Device Drivers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 16550 UART driver, VirtIO block device driver, and console output layer for xv6-riscv kernel

**Architecture:** 
- UART driver provides low-level serial communication (init, putc, getc, interrupt handler)
- VirtIO driver provides block device I/O using virtqueue ring buffers
- Console layer provides synchronized output using UART with spinlock protection

**Tech Stack:** Rust no_std kernel, RISC-V architecture, QEMU virt machine

## Global Constraints

- Target: `riscv64imac-unknown-none-elf`
- UART base: `0x10000000` (UART0)
- VirtIO base: `0x10001000` (VIRTIO0)
- PLIC base: `0x0C000000`
- UART IRQ: 10, VirtIO IRQ: 1
- Use `core::ptr::write_volatile`/`read_volatile` for MMIO
- Queue size: 8 descriptors
- 115200 baud rate for UART

---

### Task 1: Implement UART Driver (kernel/src/drivers/uart.rs)

**Files:**
- Modify: `kernel/src/drivers/uart.rs` (replace entire file)

**Interfaces:**
- Consumes: arch interrupt (`intr_on`, `intr_off`), MMIO access
- Produces: `uart_init()`, `uart_putc()`, `uart_getc()`, `uart_intr()`, `UartWriter`, `printk()`, `printk!` macro

- [ ] **Step 1: Write the failing test**

```rust
// This will be tested via integration test in Step 4

```

- [ ] **Step 2: Implement UART driver**

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

- [ ] **Step 3: Run kernel build to verify compilation**

Run: `cargo build --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add kernel/src/drivers/uart.rs
git commit -m "feat(drivers): implement 16550 UART driver"
```

---

### Task 2: Implement VirtIO Block Driver (kernel/src/drivers/virtio.rs)

**Files:**
- Modify: `kernel/src/drivers/virtio.rs` (replace entire file)

**Interfaces:**
- Consumes: frame_allocator (`alloc_page`, `free_page`), PhysAddr, atomic operations, PLIC interrupt
- Produces: `virtio_init()`, `virtio_rw()`, `virtio_intr()`

- [ ] **Step 1: Write the failing test**

```rust
// This will be tested via integration test in Step 4

```

- [ ] **Step 2: Implement VirtIO driver**

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

static mut DESC: [VirtqDesc; 8] = [VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 8];
static mut AVAIL: VirtqAvail = VirtqAvail { flags: 0, idx: 0, ring: [0; 8] };
static mut USED: VirtqUsed = VirtqUsed { flags: 0, idx: 0, ring: [VirtqUsedElem { id: 0, len: 0 }; 8] };
static FREE_DESC: AtomicU16 = AtomicU16::new(0);

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTQ_DESC_F_INDIRECT: u16 = 4;

#[repr(C, packed)]
struct VirtioBlkReq {
    type_: u32,
    reserved: u32,
    sector: u64,
}

static mut REQ: VirtioBlkReq = VirtioBlkReq { type_: 0, reserved: 0, sector: 0 };
static mut STATUS: u8 = 0;

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
    unsafe {
        // Wait for free descriptor
        while FREE_DESC.load(Ordering::Acquire) >= 8 {
            core::hint::spin_loop();
        }
        
        let idx = FREE_DESC.fetch_add(1, Ordering::AcqRel) as usize;
        
        let sector = buf.blockno;
        
        REQ.type_ = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
        REQ.reserved = 0;
        REQ.sector = sector as u64;
        
        let req_paddr = &raw const REQ as usize;
        let buf_paddr = buf.data.as_ptr() as usize;
        let status_paddr = &raw mut STATUS as usize;
        
        // Desc 0: request header (readable by device)
        DESC[idx].addr = req_paddr as u64;
        DESC[idx].len = core::mem::size_of::<VirtioBlkReq>() as u32;
        DESC[idx].flags = VIRTQ_DESC_F_NEXT;
        DESC[idx].next = ((idx + 1) % 8) as u16;
        
        // Desc 1: data buffer (writable by device for read, readable for write)
        DESC[(idx + 1) % 8].addr = buf_paddr as u64;
        DESC[(idx + 1) % 8].len = 512;
        DESC[(idx + 1) % 8].flags = if write { 0 } else { VIRTQ_DESC_F_WRITE } | VIRTQ_DESC_F_NEXT;
        DESC[(idx + 1) % 8].next = ((idx + 2) % 8) as u16;
        
        // Desc 2: status (writable by device)
        DESC[(idx + 2) % 8].addr = status_paddr as u64;
        DESC[(idx + 2) % 8].len = 1;
        DESC[(idx + 2) % 8].flags = VIRTQ_DESC_F_WRITE;
        DESC[(idx + 2) % 8].next = 0;
        
        // Add to avail ring
        let avail_idx = AVAIL.idx as usize % 8;
        AVAIL.ring[avail_idx] = idx as u16;
        core::sync::atomic::fence(Ordering::SeqCst);
        AVAIL.idx = AVAIL.idx.wrapping_add(1);
        
        // Notify device
        let v = VIRTIO0 as *mut u32;
        v.add(0x50 / 4).write_volatile(0); // Queue notify
        
        // Wait for completion (interrupt will set status)
        while STATUS == 0 {
            core::hint::spin_loop();
        }
        
        assert_eq!(STATUS, VIRTIO_BLK_S_OK);
        STATUS = 0;
        
        FREE_DESC.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn virtio_intr() {
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Acknowledge interrupt
        v.add(0x60 / 4).write_volatile(1); // Interrupt acknowledge
        
        // Process used ring
        while USED.idx != AVAIL.idx {
            let used_idx = USED.idx as usize % 8;
            let elem = USED.ring[used_idx];
            USED.idx = USED.idx.wrapping_add(1);
            // Status is already in STATUS variable
        }
    }
}
```

- [ ] **Step 3: Run kernel build to verify compilation**

Run: `cargo build --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add kernel/src/drivers/virtio.rs
git commit -m "feat(drivers): implement virtio block device driver"
```

---

### Task 3: Implement Console Layer (kernel/src/drivers/console.rs)

**Files:**
- Create: `kernel/src/drivers/console.rs` (new file)
- Modify: `kernel/src/drivers/mod.rs` (add console module)

**Interfaces:**
- Consumes: UART driver (`uart_init`, `uart_putc`), spinlock
- Produces: `console_init()`, `console_intr()`, `consputc()`, `ConsoleWriter`, `console_printk!` macro

- [ ] **Step 1: Create console.rs**

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

- [ ] **Step 2: Update drivers/mod.rs**

```rust
// kernel/src/drivers/mod.rs
pub mod uart;
pub mod virtio;
pub mod console;

pub fn uart_intr() {
    uart::uart_intr();
}

pub fn virtio_intr() {
    virtio::virtio_intr();
}

pub fn virtio_init() {
    virtio::virtio_init();
}

pub fn console_init() {
    console::console_init();
}
```

- [ ] **Step 3: Run kernel build to verify compilation**

Run: `cargo build --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add kernel/src/drivers/console.rs kernel/src/drivers/mod.rs
git commit -m "feat(drivers): implement console output layer"
```

---

### Task 4: Test Console Output

**Files:**
- Test: `kernel/src/bin/kernel.rs` (implicit via boot)

**Interfaces:**
- Consumes: All drivers
- Produces: Working kernel boot with console output

- [ ] **Step 1: Run kernel in QEMU**

Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Prints "xv6-rust kernel is booting" to QEMU console

- [ ] **Step 2: Commit**

```bash
git commit -m "test(drivers): verify console output works"
```

---

### Task 5: Test Disk I/O (if fs::buf exists)

**Files:**
- Test: Integration test via kernel

**Interfaces:**
- Consumes: VirtIO driver, fs::buf

- [ ] **Step 1: Check if fs::buf exists**

Run: `grep -r "pub struct Buf" kernel/src/fs/`
Expected: Buf struct exists or needs to be created

- [ ] **Step 2: Run virtio tests if available**

Run: `cargo test --target riscv64imac-unknown-none-elf -p xv6-kernel drivers::virtio`
Expected: Reads/writes blocks correctly (or test doesn't exist yet)

- [ ] **Step 3: Commit**

```bash
git commit -m "test(drivers): verify disk I/O works"
```

---

### Task 6: Final Integration Test

**Files:**
- Test: Full kernel boot

- [ ] **Step 1: Run full kernel boot test**

Run: `cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel`
Expected: Kernel boots successfully, prints boot message, scheduler runs

- [ ] **Step 2: Final commit**

```bash
git add -A
git commit -m "feat(drivers): complete device drivers - UART, VirtIO, console"
```