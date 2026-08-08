// kernel/src/drivers/virtio.rs
//! Virtio block device driver.
//!
//! Implements the virtio 1.0 block device interface using memory-mapped I/O
//! at VIRTIO0 (0x10001000). Uses a simple single-queue design with 8 descriptors.

use crate::arch::asm::VIRTIO0;
use crate::arch::interrupt::plic_init_hart;
use crate::mm::frame_allocator::alloc_page;
use crate::sync::mutex::Mutex;
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
const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x50;
const VIRTIO_MMIO_INTERRUPT_ACK: usize = 0x60;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_S_OK: u8 = 0;
const VIRTIO_BLK_S_IOERR: u8 = 1;
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

const VIRTIO_STATUS_ACKNOWLEDGE: u32 = 1;
const VIRTIO_STATUS_DRIVER: u32 = 2;
const VIRTIO_STATUS_DRIVER_OK: u32 = 4;
const VIRTIO_STATUS_FEATURES_OK: u32 = 8;

const QUEUE_SIZE: usize = 8;

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
    ring: [u16; QUEUE_SIZE],
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
    ring: [VirtqUsedElem; QUEUE_SIZE],
}

const VIRTQ_DESC_F_NEXT: u16 = 1;
const VIRTQ_DESC_F_WRITE: u16 = 2;
const VIRTQ_DESC_F_INDIRECT: u16 = 4;

#[repr(C, packed)]
struct VirtioBlkReq {
    type_: u32,
    reserved: u32,
    sector: u64,
}

/// Virtio block device state.
/// 
/// Contains all ring buffers and synchronization primitives needed
/// for virtio operations. Protected by a mutex for concurrent access.
struct VirtioDevice {
    desc: [VirtqDesc; QUEUE_SIZE],
    avail: VirtqAvail,
    used: VirtqUsed,
    free_desc: AtomicU16,
    req: VirtioBlkReq,
    status: u8,
    // Physical addresses of the allocated pages
    _desc_page: usize,
    _avail_page: usize,
    _used_page: usize,
}

impl VirtioDevice {
    fn new(
        desc_page: usize,
        avail_page: usize,
        used_page: usize,
    ) -> Self {
        Self {
            desc: [VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; QUEUE_SIZE],
            avail: VirtqAvail { flags: 0, idx: 0, ring: [0; QUEUE_SIZE] },
            used: VirtqUsed { flags: 0, idx: 0, ring: [VirtqUsedElem { id: 0, len: 0 }; QUEUE_SIZE] },
            free_desc: AtomicU16::new(0),
            req: VirtioBlkReq { type_: 0, reserved: 0, sector: 0 },
            status: 0,
            _desc_page: desc_page,
            _avail_page: avail_page,
            _used_page: used_page,
        }
    }
}

/// Global virtio device instance.
/// 
/// Initialized by `virtio_init()` and accessed via `VIRTIO_DEVICE.lock()`.
static VIRTIO_DEVICE: Mutex<Option<VirtioDevice>> = Mutex::new(None);

/// Initialize the virtio block device.
/// 
/// Negotiates features, allocates queue pages, and enables interrupts.
pub fn virtio_init() {
    // Try multiple common virtio MMIO addresses for RISC-V virt machine
    const VIRTIO_BASES: [usize; 8] = [
        0x10001000, 0x10002000, 0x10003000, 0x10004000,
        0x10005000, 0x10006000, 0x10007000, 0x10008000,
    ];
    
    let mut v = core::ptr::null_mut();
    for base in VIRTIO_BASES {
        unsafe {
            let magic = (base as *mut u32).add(VIRTIO_MMIO_MAGIC_VALUE / 4).read_volatile();
            let version = (base as *mut u32).add(VIRTIO_MMIO_VERSION / 4).read_volatile();
            let device_id = (base as *mut u32).add(VIRTIO_MMIO_DEVICE_ID / 4).read_volatile();
            
            if magic == 0x74726976 && (version == 1 || version == 2) && device_id == 2 {
                v = base as *mut u32;
                break;
            }
        }
    }
    
    if v.is_null() {
        crate::arch::console::printk(format_args!("virtio: no block device found!\n"));
        return;
    }
    
    unsafe {
        // Verify device
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
        if max < 8 {
            crate::arch::console::printk(format_args!("virtio: max queue too small: {}\n", max));
            return;
        }
        v.add(VIRTIO_MMIO_QUEUE_NUM / 4).write_volatile(8);

        // Allocate queue pages
        let desc_page = alloc_page().expect("virtio desc");
        let avail_page = alloc_page().expect("virtio avail");
        let used_page = alloc_page().expect("virtio used");
        
        // Store physical addresses in device registers
        v.add(VIRTIO_MMIO_QUEUE_DESC_LOW / 4).write_volatile((desc_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_DESC_HIGH / 4).write_volatile((desc_page.0 >> 20) as u32);
        v.add(VIRTIO_MMIO_QUEUE_AVAIL_LOW / 4).write_volatile((avail_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_AVAIL_HIGH / 4).write_volatile((avail_page.0 >> 20) as u32);
        v.add(VIRTIO_MMIO_QUEUE_USED_LOW / 4).write_volatile((used_page.0 << 12) as u32);
        v.add(VIRTIO_MMIO_QUEUE_USED_HIGH / 4).write_volatile((used_page.0 >> 20) as u32);
        
        v.add(VIRTIO_MMIO_QUEUE_READY / 4).write_volatile(1);

        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);

        // Initialize device state
        let device = VirtioDevice::new(desc_page.0 << 12, avail_page.0 << 12, used_page.0 << 12);
        *VIRTIO_DEVICE.lock() = Some(device);
        
        // Enable interrupt
        plic_init_hart();
    }
}

/// Block buffer for virtio read/write operations.
///
/// `blockno` is the starting 512-byte sector; `data` is transferred in a single
/// request, so a 1024-byte buffer moves two consecutive sectors at once (the FS
/// block size). virtio-blk transfers `data.len() / 512` sectors per request.
pub struct Block {
    pub blockno: u64,
    pub data: [u8; 1024],
}

/// Read or write a block via virtio.
///
/// # Arguments
/// * `block` - Buffer containing sector number and data
/// * `write` - `true` for write, `false` for read
///
/// Blocks until the operation completes.
pub fn virtio_rw(block: &mut Block, write: bool) {
    virtio_rw_buf(block.blockno, &mut block.data, write);
}

/// Read or write `buf.len() / 512` consecutive sectors starting at `sector` in a
/// single virtio request. `buf` must be physically contiguous (identity-mapped
/// kernel memory) and its length a multiple of 512. Used to collapse several
/// consecutive on-disk blocks (e.g. the log area) into one round-trip instead of
/// one request per block.
///
/// Blocks until the operation completes.
pub fn virtio_rw_buf(sector: u64, buf: &mut [u8], write: bool) {
    // Hold the device lock for the whole operation: this serialises requests so
    // only one is ever outstanding, which lets us use a fixed 3-descriptor chain
    // (0 -> 1 -> 2) and a simple polled completion.
    let mut device_guard = VIRTIO_DEVICE.lock();
    let device = device_guard.as_mut().expect("virtio not initialized");

    // CRITICAL: the descriptor table, avail ring and used ring the DEVICE reads
    // and writes live in the physical pages we programmed into the queue
    // registers at init (QUEUE_DESC/AVAIL/USED). They are NOT the arrays that
    // happen to sit inside `VirtioDevice`. The kernel identity-maps physical
    // memory, so the stored page addresses are usable directly as pointers.
    let desc = device._desc_page as *mut VirtqDesc;
    let avail = device._avail_page as *mut VirtqAvail;
    let used = device._used_page as *const VirtqUsed;

    // Request header and status live in kernel .bss (inside the device struct),
    // which is identity-mapped, so the device can DMA to their addresses.
    device.req.type_ = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
    device.req.reserved = 0;
    device.req.sector = sector;
    device.status = 0xff; // sentinel; device overwrites with 0 (OK) on success

    let req_pa = &raw const device.req as u64;
    let buf_pa = buf.as_ptr() as u64;
    let buf_len = buf.len() as u32;
    let status_pa = &raw mut device.status as u64;

    unsafe {
        // desc[0]: request header, device-readable, chains to data.
        (*desc.add(0)).addr = req_pa;
        (*desc.add(0)).len = core::mem::size_of::<VirtioBlkReq>() as u32;
        (*desc.add(0)).flags = VIRTQ_DESC_F_NEXT;
        (*desc.add(0)).next = 1;

        // desc[1]: data buffer (buf_len bytes = that many /512 sectors).
        // Device-WRITABLE on read, device-readable on write. Chains to status.
        (*desc.add(1)).addr = buf_pa;
        (*desc.add(1)).len = buf_len;
        (*desc.add(1)).flags = VIRTQ_DESC_F_NEXT | if write { 0 } else { VIRTQ_DESC_F_WRITE };
        (*desc.add(1)).next = 2;

        // desc[2]: 1-byte status, device-writable, end of chain.
        (*desc.add(2)).addr = status_pa;
        (*desc.add(2)).len = 1;
        (*desc.add(2)).flags = VIRTQ_DESC_F_WRITE;
        (*desc.add(2)).next = 0;

        // Publish the chain head (descriptor 0) in the avail ring, then bump idx.
        let first_used = core::ptr::read_volatile(&(*used).idx);
        let ai = (*avail).idx;
        (*avail).ring[(ai as usize) % QUEUE_SIZE] = 0;
        core::sync::atomic::fence(Ordering::SeqCst);
        (*avail).idx = ai.wrapping_add(1);
        core::sync::atomic::fence(Ordering::SeqCst);

        // Notify the device (queue 0).
        let v = VIRTIO0 as *mut u32;
        v.add(VIRTIO_MMIO_QUEUE_NOTIFY / 4).write_volatile(0);

        // Poll until the device advances the used ring past where it was.
        while core::ptr::read_volatile(&(*used).idx) == first_used {
            core::hint::spin_loop();
        }

        let st = core::ptr::read_volatile(&raw const device.status);
        assert_eq!(st, VIRTIO_BLK_S_OK, "virtio_rw: device reported error status");
    }
}

/// Virtio interrupt handler.
/// 
/// Acknowledges the interrupt and processes the used ring.
pub fn virtio_intr() {
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Acknowledge interrupt
        v.add(VIRTIO_MMIO_INTERRUPT_ACK / 4).write_volatile(1);
    }
    
    let mut device_guard = VIRTIO_DEVICE.lock();
    let device = device_guard.as_mut().expect("virtio not initialized");
    
    // Process used ring
    while device.used.idx != device.avail.idx {
        let used_idx = device.used.idx as usize % QUEUE_SIZE;
        let _elem = device.used.ring[used_idx];
        device.used.idx = device.used.idx.wrapping_add(1);
        // Read status from device-written memory location
        let status = unsafe { core::ptr::read_volatile(&raw const device.status) };
        if status != 0 {
            unsafe { core::ptr::write_volatile(&raw mut device.status, status); }
        }
    }
}