// kernel/src/fs/buf.rs
use crate::drivers::virtio::virtio_rw;
use crate::sync::spinlock::SpinLock;
use crate::sync::sleeplock::SleepLock;
use crate::sync::condvar::Condvar;
use crate::proc::{sleep, wakeup};

pub const BSIZE: usize = 1024;

struct BufData {
    blockno: u32,
    dev: u32,
    refcnt: usize,
    valid: bool,
    data: [u8; BSIZE],
}

pub struct Buf {
    lock: SleepLock<BufData>,
}

impl Buf {
    pub fn new(blockno: u32, dev: u32) -> Self {
        Self {
            lock: SleepLock::new(BufData {
                blockno,
                dev,
                refcnt: 0,
                valid: false,
                data: [0; BSIZE],
            }, "buf"),
        }
    }
    
    pub fn lock(&self) -> BufGuard<'_> { 
        BufGuard { guard: self.lock.acquire() }
    }
}

pub struct BufGuard<'a> {
    guard: crate::sync::sleeplock::SleepLockGuard<'a, BufData>,
}

impl<'a> BufGuard<'a> {
    pub fn data(&self) -> &[u8] { &self.guard.data }
    pub fn data_mut(&mut self) -> &mut [u8] { &mut self.guard.data }
    pub fn blockno(&self) -> u32 { self.guard.blockno }
    pub fn dev(&self) -> u32 { self.guard.dev }
    pub fn valid(&self) -> bool { self.guard.valid }
    pub fn set_valid(&mut self, v: bool) { self.guard.valid = v; }
    pub fn refcnt(&self) -> usize { self.guard.refcnt }
    pub fn inc_ref(&mut self) { self.guard.refcnt += 1; }
    pub fn dec_ref(&mut self) { self.guard.refcnt -= 1; }
}

// Buffer reference for safe access - stores index and we look up the buffer when needed
#[derive(Clone, Copy)]
pub struct BufRef {
    index: usize,
}

impl BufRef {
    pub fn new(index: usize) -> Self { Self { index } }
    pub fn index(&self) -> usize { self.index }
    
    pub fn lock(&self) -> BufGuard<'_> {
        let cache = BUF_CACHE.acquire();
        let buf = cache.buffers[self.index].as_ref().unwrap();
        buf.lock()
    }
}

const NBUF: usize = 30; // MAXOPBLOCKS * 3 = 10 * 3 = 30

pub static BUF_CACHE: SpinLock<BufCache> = SpinLock::new(BufCache::new(), "bcache");

struct BufCache {
    buffers: [Option<Buf>; NBUF],
    head: Option<usize>, // LRU list - index of most recently used
    tail: Option<usize>, // LRU list - index of least recently used
    prev: [Option<usize>; NBUF], // Previous buffer in LRU
    next: [Option<usize>; NBUF], // Next buffer in LRU
    wait_cond: Condvar,
}

impl BufCache {
    const fn new() -> Self { 
        Self { 
            buffers: [const { None }; NBUF], 
            head: None,
            tail: None,
            prev: [const { None }; NBUF],
            next: [const { None }; NBUF],
            wait_cond: Condvar::new("bcache"),
        } 
    }
    
    fn remove_from_lru(&mut self, idx: usize) {
        let p = self.prev[idx];
        let n = self.next[idx];
        
        if let Some(p) = p {
            self.next[p] = n;
        } else {
            self.head = n;
        }
        
        if let Some(n) = n {
            self.prev[n] = p;
        } else {
            self.tail = p;
        }
        
        self.prev[idx] = None;
        self.next[idx] = None;
    }
    
    fn add_to_head(&mut self, idx: usize) {
        self.next[idx] = self.head;
        self.prev[idx] = None;
        
        if let Some(h) = self.head {
            self.prev[h] = Some(idx);
        }
        
        self.head = Some(idx);
        
        if self.tail.is_none() {
            self.tail = Some(idx);
        }
    }
    
    fn move_to_head(&mut self, idx: usize) {
        if self.head == Some(idx) {
            return; // Already at head
        }
        self.remove_from_lru(idx);
        self.add_to_head(idx);
    }
    
    fn get_lru(&self) -> Option<usize> {
        self.tail
    }
}

pub fn binit() {
    let mut cache = BUF_CACHE.acquire();
    for i in 0..NBUF {
        cache.buffers[i] = Some(Buf::new(0, 0));
    }
    // Initialize LRU with all buffers
    for i in 0..NBUF {
        cache.add_to_head(i);
    }
}

fn bget(dev: u32, blockno: u32) -> BufRef {
    loop {
        let mut cache = BUF_CACHE.acquire();
        
        // Search for existing buffer
        let mut found_idx = None;
        for i in 0..NBUF {
            if let Some(buf) = &cache.buffers[i] {
                let mut guard = buf.lock();
                if guard.dev() == dev && guard.blockno() == blockno {
                    guard.inc_ref();
                    found_idx = Some(i);
                    break;
                }
            }
        }
        
        if let Some(i) = found_idx {
            cache.move_to_head(i);
            return BufRef::new(i);
        }
        
        // Not found - find a free buffer (refcnt == 0)
        if let Some(lru_idx) = cache.get_lru() {
            if let Some(buf) = &cache.buffers[lru_idx] {
                let mut guard = buf.lock();
                if guard.refcnt() == 0 {
                    // Reuse this buffer
                    guard.guard.blockno = blockno;
                    guard.guard.dev = dev;
                    guard.guard.valid = false;
                    guard.guard.refcnt = 1;
                    cache.move_to_head(lru_idx);
                    return BufRef::new(lru_idx);
                }
            }
        }
        
        // All buffers busy - sleep and retry
        cache.wait_cond.sleep(&BUF_CACHE);
        // After wakeup, loop and try again
    }
}

pub fn bread(dev: u32, blockno: u32) -> BufRef {
    let buf_ref = bget(dev, blockno);
    {
        let mut cache = BUF_CACHE.acquire();
        let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
        let mut guard = buf.lock();
        
        if !guard.valid() {
            // Need to read from disk
            drop(cache); // Release lock before I/O
            read_block(dev, blockno, guard.data_mut());
            guard.set_valid(true);
        }
    }
    buf_ref
}

pub fn brelse(buf_ref: BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.dec_ref();
    let refcnt = guard.refcnt();
    drop(guard);
    if refcnt == 0 {
        // Move to head of LRU (most recently used)
        cache.move_to_head(buf_ref.index());
        cache.wait_cond.wakeup();
    }
}

pub fn bwrite(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    if guard.valid() {
        let dev = guard.dev();
        let blockno = guard.blockno();
        let data = guard.data().to_vec();
        drop(guard);
        drop(cache);
        write_block(dev, blockno, &data);
    }
}

pub fn bpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.inc_ref();
}

pub fn bunpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
    let mut guard = buf.lock();
    guard.dec_ref();
    let refcnt = guard.refcnt();
    drop(guard);
    if refcnt == 0 {
        cache.move_to_head(buf_ref.index());
        cache.wait_cond.wakeup();
    }
}

// Read a 1024-byte block from disk (2x 512-byte sectors)
fn read_block(dev: u32, blockno: u32, dst: &mut [u8]) {
    // Virtio uses 512-byte sectors, we need 2 sectors for 1024-byte block
    for i in 0..2usize {
        let sector = (blockno as u64) * 2 + i as u64;
        let mut block = crate::drivers::virtio::Block {
            blockno: sector,
            data: [0u8; 512],
        };
        virtio_rw(&mut block, false); // false = read
        let start = i * 512;
        let end = (i + 1) * 512;
        dst[start..end].copy_from_slice(&block.data);
    }
}

// Write a 1024-byte block to disk (2x 512-byte sectors)
fn write_block(dev: u32, blockno: u32, src: &[u8]) {
    for i in 0..2usize {
        let sector = (blockno as u64) * 2 + i as u64;
        let mut block = crate::drivers::virtio::Block {
            blockno: sector,
            data: [0u8; 512],
        };
        let start = i * 512;
        let end = (i + 1) * 512;
        block.data.copy_from_slice(&src[start..end]);
        virtio_rw(&mut block, true); // true = write
    }
}