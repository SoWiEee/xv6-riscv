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
    crate::arch::console::printk(format_args!("virtio: initializing...\n"));
    unsafe {
        let v = VIRTIO0 as *mut u32;
        // Verify device
        let magic = v.add(VIRTIO_MMIO_MAGIC_VALUE / 4).read_volatile();
        let version = v.add(VIRTIO_MMIO_VERSION / 4).read_volatile();
        let device_id = v.add(VIRTIO_MMIO_DEVICE_ID / 4).read_volatile();
        
        crate::arch::console::printk(format_args!("virtio: magic={:#x} version={} device_id={}\n", magic, version, device_id));
        
        if magic != 0x74726976 || (version != 1 && version != 2) || device_id != 2 {
            crate::arch::console::printk(format_args!("virtio: device check failed!\n"));
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
        
        crate::arch::console::printk(format_args!("virtio: queue setup...\n"));
        
        // Queue setup
        v.add(VIRTIO_MMIO_QUEUE_SEL / 4).write_volatile(0);
        let max = v.add(VIRTIO_MMIO_QUEUE_NUM_MAX / 4).read_volatile();
        crate::arch::console::printk(format_args!("virtio: max queue={}\n", max));
        if max < 8 {
            crate::arch::console::printk(format_args!("virtio: max queue too small: {}\n", max));
            return;
        }
        v.add(VIRTIO_MMIO_QUEUE_NUM / 4).write_volatile(8);
        crate::arch::console::printk(format_args!("virtio: queue num set\n"));
        
        // Allocate queue pages
        let desc_page = alloc_page().expect("virtio desc");
        crate::arch::console::printk(format_args!("virtio: desc page={:#x}\n", desc_page.0 << 12));
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
        crate::arch::console::printk(format_args!("virtio: queue ready\n"));

        v.add(VIRTIO_MMIO_STATUS / 4).write_volatile(VIRTIO_STATUS_ACKNOWLEDGE | VIRTIO_STATUS_DRIVER | VIRTIO_STATUS_FEATURES_OK | VIRTIO_STATUS_DRIVER_OK);

        crate::arch::console::printk(format_args!("virtio: device initialized\n"));

        // Initialize device state
        let device = VirtioDevice::new(desc_page.0 << 12, avail_page.0 << 12, used_page.0 << 12);
        *VIRTIO_DEVICE.lock() = Some(device);
        
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
    crate::arch::console::printk(format_args!("virtio_rw: block={} write={}\n", block.blockno, write));
    // Wait for free descriptor
    let idx = loop {
        let mut device_guard = VIRTIO_DEVICE.lock();
        let device = device_guard.as_mut().expect("virtio not initialized");
        
        if device.free_desc.load(Ordering::Acquire) < QUEUE_SIZE as u16 {
            break device.free_desc.fetch_add(1, Ordering::AcqRel) as usize;
        }
        drop(device_guard);
        core::hint::spin_loop();
    };
    
    crate::arch::console::printk(format_args!("virtio_rw: got desc idx={}\n", idx));
    // Wait for free descriptor
    let idx = loop {
        let mut device_guard = VIRTIO_DEVICE.lock();
        let device = device_guard.as_mut().expect("virtio not initialized");
        
        if device.free_desc.load(Ordering::Acquire) < QUEUE_SIZE as u16 {
            break device.free_desc.fetch_add(1, Ordering::AcqRel) as usize;
        }
        drop(device_guard);
        core::hint::spin_loop();
    };
    
    let sector = block.blockno;
    
    {
        let mut device_guard = VIRTIO_DEVICE.lock();
        let mut device = device_guard.as_mut().expect("virtio not initialized");
        
        device.req.type_ = if write { VIRTIO_BLK_T_OUT } else { VIRTIO_BLK_T_IN };
        device.req.reserved = 0;
        device.req.sector = sector;
        
        let req_paddr = &raw const device.req as usize;
        let buf_paddr = block.data.as_ptr() as usize;
        let status_paddr = &raw mut device.status as usize;
        
        // Desc 0: request header (readable by device)
        device.desc[idx].addr = req_paddr as u64;
        device.desc[idx].len = core::mem::size_of::<VirtioBlkReq>() as u32;
        device.desc[idx].flags = VIRTQ_DESC_F_NEXT;
        device.desc[idx].next = ((idx + 1) % QUEUE_SIZE) as u16;
        
        // Desc 1: data buffer (writable by device for read, readable for write)
        device.desc[(idx + 1) % QUEUE_SIZE].addr = buf_paddr as u64;
        device.desc[(idx + 1) % QUEUE_SIZE].len = 512;
        device.desc[(idx + 1) % QUEUE_SIZE].flags = if write { 0 } else { VIRTQ_DESC_F_WRITE } | VIRTQ_DESC_F_NEXT;
        device.desc[(idx + 1) % QUEUE_SIZE].next = ((idx + 2) % QUEUE_SIZE) as u16;
        
        // Desc 2: status (writable by device)
        device.desc[(idx + 2) % QUEUE_SIZE].addr = status_paddr as u64;
        device.desc[(idx + 2) % QUEUE_SIZE].len = 1;
        device.desc[(idx + 2) % QUEUE_SIZE].flags = VIRTQ_DESC_F_WRITE;
        device.desc[(idx + 2) % QUEUE_SIZE].next = 0;
        
        // Add to avail ring
        let avail_idx = device.avail.idx as usize % QUEUE_SIZE;
        device.avail.ring[avail_idx] = idx as u16;
        core::sync::atomic::fence(Ordering::SeqCst);
        device.avail.idx = device.avail.idx.wrapping_add(1);
        
        // Notify device
        unsafe {
            let v = VIRTIO0 as *mut u32;
            v.add(VIRTIO_MMIO_QUEUE_NOTIFY / 4).write_volatile(0);
        }
        
        // Wait for completion (poll used ring)
        let mut wait_count = 0;
        loop {
            // Check if device has processed the request by checking used ring
            let used_idx_val = device.used.idx;
            let avail_idx_val = device.avail.idx;
            if used_idx_val != avail_idx_val {
                crate::arch::console::printk(format_args!("virtio_rw: used_idx={} avail_idx={} after {} loops\n", used_idx_val, avail_idx_val, wait_count));
                // Process the completed request
                let used_idx = used_idx_val as usize % QUEUE_SIZE;
                let _elem = device.used.ring[used_idx];
                device.used.idx = used_idx_val.wrapping_add(1);
                // Read status from device-written memory location
                let status = unsafe { core::ptr::read_volatile(&raw const device.status) };
                crate::arch::console::printk(format_args!("virtio_rw: read status={}\n", status));
                unsafe { core::ptr::write_volatile(&raw mut device.status, status); }
                // virtio blk: status 0 = OK, non-zero = error
                break;
            }
            // Save values needed after dropping guard
            let _used_idx_val = used_idx_val;
            let _avail_idx_val = avail_idx_val;
            drop(device_guard);
            core::hint::spin_loop();
            wait_count += 1;
            if wait_count % 1000000 == 0 {
                // Need to re-acquire to read status
                let mut tmp_guard = VIRTIO_DEVICE.lock();
                let tmp_device = tmp_guard.as_mut().expect("virtio not initialized");
                let current_status = unsafe { core::ptr::read_volatile(&raw const tmp_device.status) };
                crate::arch::console::printk(format_args!("virtio_rw: waiting... used_idx={} avail_idx={} status={}\n", _used_idx_val, _avail_idx_val, current_status));
                drop(tmp_guard);
            }
            device_guard = VIRTIO_DEVICE.lock();
            device = device_guard.as_mut().expect("virtio not initialized");
        }
        
        assert_eq!(unsafe { core::ptr::read_volatile(&raw const device.status) }, VIRTIO_BLK_S_OK);
        unsafe { core::ptr::write_volatile(&raw mut device.status, 0); }
        
        device.free_desc.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Virtio interrupt handler.
/// 
/// Acknowledges the interrupt and processes the used ring.
pub fn virtio_intr() {
    crate::arch::console::printk(format_args!("virtio_intr\n"));
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