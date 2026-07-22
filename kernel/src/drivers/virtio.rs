// kernel/src/drivers/virtio.rs
//! Virtio block device driver.
//!
//! Implements the virtio 1.0 block device interface using memory-mapped I/O
//! at VIRTIO0 (0x10001000). Uses a simple single-queue design with 8 descriptors.

use crate::arch::asm::VIRTIO0;
use crate::arch::interrupt::plic_init_hart;
use crate::mm::frame_allocator::alloc_page;
use core::sync::atomic::{AtomicU16, Ordering};

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
#[derive(Copy, Clone)]
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
#[derive(Copy, Clone)]
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

/// Initialize the virtio block device.
/// 
/// Negotiates features, allocates queue pages, and enables interrupts.
pub fn virtio_init() {
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Verify device
        let magic = v.add(VIRTIO_MMIO_MAGIC_VALUE / 4).read_volatile();
        let version = v.add(VIRTIO_MMIO_VERSION / 4).read_volatile();
        let device_id = v.add(VIRTIO_MMIO_DEVICE_ID / 4).read_volatile();
        
        if magic != 0x74726976 || version != 2 || device_id != 2 {
            return;
        }
        
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
        plic_init_hart();
    }
}

/// Block buffer for virtio read/write operations.
/// 
/// Contains a 512-byte sector buffer and the sector number.
pub struct Block {
    pub blockno: u64,
    pub data: [u8; 512],
}

/// Read or write a block via virtio.
/// 
/// # Arguments
/// * `block` - Buffer containing sector number and data
/// * `write` - `true` for write, `false` for read
/// 
/// Blocks until the operation completes.
pub fn virtio_rw(block: &mut Block, write: bool) {
    unsafe {
        // Wait for free descriptor
        while FREE_DESC.load(Ordering::Acquire) >= 8 {
            core::hint::spin_loop();
        }
        
        let idx = FREE_DESC.fetch_add(1, Ordering::AcqRel) as usize;
        
        let sector = block.blockno;
        
        REQ.type_ = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
        REQ.reserved = 0;
        REQ.sector = sector;
        
        let req_paddr = &raw const REQ as usize;
        let buf_paddr = block.data.as_ptr() as usize;
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
        while unsafe { core::ptr::read_volatile(&raw const STATUS) } == 0 {
            core::hint::spin_loop();
        }
        
        assert_eq!(unsafe { core::ptr::read_volatile(&raw const STATUS) }, VIRTIO_BLK_S_OK);
        unsafe { core::ptr::write_volatile(&raw mut STATUS, 0); }
        
        FREE_DESC.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Virtio interrupt handler.
/// 
/// Acknowledges the interrupt and processes the used ring.
pub fn virtio_intr() {
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Acknowledge interrupt
        v.add(0x60 / 4).write_volatile(1); // Interrupt acknowledge
        
        // Process used ring
        while USED.idx != AVAIL.idx {
            let used_idx = USED.idx as usize % 8;
            let _elem = USED.ring[used_idx];
            USED.idx = USED.idx.wrapping_add(1);
            // Status is already in STATUS variable
        }
    }
}