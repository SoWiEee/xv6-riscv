// kernel/src/fs/buf.rs
//! Buffer cache for disk blocks.
//!
//! Implements a buffer cache with LRU eviction. Each buffer holds one
//! 1024-byte disk block (two 512-byte virtio sectors).
//!
//! Locking discipline mirrors xv6 (`kernel/bio.c`):
//!   * The buffer *identity* (`dev`, `blockno`, `refcnt`) and the LRU list are
//!     protected by the `BUF_CACHE` **spinlock**. `bget` scans and evicts while
//!     holding only this spinlock — it never touches a sleeplock during the scan.
//!   * The buffer *contents* (`valid`, `data`) are protected by a **sleeplock**,
//!     acquired only after the cache spinlock has been released.
//!
//! Keeping identity out of the sleeplock is what lets `bget` search the cache
//! without ever sleeping while holding a spinlock (which would be illegal).

use crate::drivers::virtio::virtio_rw;
use crate::sync::spinlock::SpinLock;
use crate::sync::sleeplock::SleepLock;

/// Block size in bytes (1024 = 2 virtio sectors).
pub const BSIZE: usize = 1024;

/// Buffer contents, protected by the per-buffer sleeplock.
struct BufData {
    valid: bool,
    data: alloc::vec::Vec<u8>,
}

/// A cached disk block.
///
/// `dev`/`blockno`/`refcnt` are the buffer's identity and are only ever touched
/// while holding the `BUF_CACHE` spinlock. `inner` (valid + data) is guarded by
/// its own sleeplock because I/O on it may block.
pub struct Buf {
    dev: u32,
    blockno: u32,
    refcnt: usize,
    inner: SleepLock<BufData>,
}

impl Buf {
    /// Create a new uninitialized buffer.
    pub fn new(blockno: u32, dev: u32) -> Self {
        Self {
            dev,
            blockno,
            refcnt: 0,
            inner: SleepLock::new(
                BufData {
                    valid: false,
                    data: alloc::vec![0u8; BSIZE],
                },
                "buf",
            ),
        }
    }
}

/// RAII guard for a locked buffer's contents.
///
/// Provides access to the block data; the identity fields live under the cache
/// spinlock and are not reachable from here.
pub struct BufGuard<'a> {
    guard: crate::sync::sleeplock::SleepLockGuard<'a, BufData>,
}

impl<'a> BufGuard<'a> {
    /// Get immutable reference to buffer data.
    pub fn data(&self) -> &[u8] { &self.guard.data }

    /// Get mutable reference to buffer data.
    pub fn data_mut(&mut self) -> &mut [u8] { &mut self.guard.data }

    /// Check if buffer contains valid (disk-loaded) data.
    pub fn valid(&self) -> bool { self.guard.valid }

    /// Set validity flag.
    pub fn set_valid(&mut self, v: bool) { self.guard.valid = v; }
}

/// Reference to a buffer in the cache.
///
/// Holds an index into the global buffer cache. The buffer's contents are
/// reached via `lock()`, which acquires only the sleeplock.
#[derive(Clone, Copy)]
pub struct BufRef {
    index: usize,
}

impl BufRef {
    /// Create a new buffer reference.
    pub fn new(index: usize) -> Self { Self { index } }

    /// Get the cache index.
    pub fn index(&self) -> usize { self.index }

    /// Get the disk block number this buffer currently holds.
    ///
    /// Reads the spinlock-protected identity field. Used by the log layer to
    /// record which home block a logged buffer belongs to.
    pub fn blockno(&self) -> u32 {
        let cache = BUF_CACHE.acquire();
        cache.buffers[self.index].as_ref().unwrap().blockno
    }

    /// Lock the referenced buffer's contents (sleeplock only).
    pub fn lock(&self) -> BufGuard<'_> {
        // SAFETY: BUF_CACHE is a 'static, so the Buf lives for the whole
        // program. We take the cache spinlock only to read a stable pointer to
        // the Buf, then release it before touching the sleeplock — so we never
        // hold the spinlock across a potentially-blocking sleeplock acquire.
        let cache = BUF_CACHE.acquire();
        let buf_ptr: *const Buf = cache.buffers[self.index].as_ref().unwrap() as *const Buf;
        drop(cache);
        BufGuard { guard: unsafe { &*buf_ptr }.inner.acquire() }
    }
}

/// Number of buffers in the cache.
const NBUF: usize = 30; // MAXOPBLOCKS * 3 = 10 * 3 = 30

/// Global buffer cache.
pub static BUF_CACHE: SpinLock<BufCache> = SpinLock::new(BufCache::new(), "bcache");

pub fn bcache_addr() -> usize {
    &raw const BUF_CACHE as usize
}

/// Buffer cache with LRU replacement policy.
struct BufCache {
    buffers: [Option<Buf>; NBUF],
    head: Option<usize>, // LRU list - index of most recently used
    tail: Option<usize>, // LRU list - index of least recently used
    prev: [Option<usize>; NBUF], // Previous buffer in LRU
    next: [Option<usize>; NBUF], // Next buffer in LRU
}

impl BufCache {
    const fn new() -> Self {
        Self {
            buffers: [const { None }; NBUF],
            head: None,
            tail: None,
            prev: [const { None }; NBUF],
            next: [const { None }; NBUF],
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
}

/// Initialize the buffer cache.
pub fn binit() {
    let mut cache = BUF_CACHE.acquire();
    for i in 0..NBUF {
        cache.buffers[i] = Some(Buf::new(0, 0));
    }
    // Initialize LRU with all buffers.
    for i in 0..NBUF {
        cache.add_to_head(i);
    }
}

/// Get a buffer for a disk block, allocating or evicting as needed.
///
/// Returns a `BufRef` with `refcnt` incremented (pinned). The scan touches only
/// the spinlock-protected identity fields — no sleeplock is acquired here.
fn bget(dev: u32, blockno: u32) -> BufRef {
    let mut cache = BUF_CACHE.acquire();

    loop {
        // Is the block already cached?
        for i in 0..NBUF {
            if let Some(buf) = &cache.buffers[i] {
                if buf.dev == dev && buf.blockno == blockno {
                    cache.buffers[i].as_mut().unwrap().refcnt += 1;
                    cache.move_to_head(i);
                    return BufRef::new(i);
                }
            }
        }

        // Not cached: recycle the least-recently-used unused buffer. Walk the
        // LRU list from the tail (oldest) toward the head looking for refcnt==0.
        let mut cur = cache.tail;
        while let Some(idx) = cur {
            let is_free = matches!(&cache.buffers[idx], Some(b) if b.refcnt == 0);
            if is_free {
                {
                    let buf = cache.buffers[idx].as_mut().unwrap();
                    buf.dev = dev;
                    buf.blockno = blockno;
                    buf.refcnt = 1;
                }
                cache.move_to_head(idx);
                // Release the cache spinlock BEFORE touching the sleeplock, then
                // invalidate the recycled contents so a stale block from the
                // previous tenant is never mistaken for valid data.
                drop(cache);
                let bref = BufRef::new(idx);
                bref.lock().set_valid(false);
                return bref;
            }
            cur = cache.prev[idx];
        }

        // Every buffer is busy. Like xv6, this is fatal: NBUF is sized for the
        // maximum number of simultaneously-pinned blocks, so exhausting it means
        // a leak (a missing brelse), not transient contention.
        panic!("bget: no buffers");
    }
}

/// Read a disk block into the cache.
///
/// Returns a `BufRef` (pinned) whose contents are valid. If the block was not
/// already cached it is read from disk.
pub fn bread(dev: u32, blockno: u32) -> BufRef {
    let buf_ref = bget(dev, blockno);
    let mut guard = buf_ref.lock();
    if !guard.valid() {
        read_block(dev, blockno, guard.data_mut());
        guard.set_valid(true);
    }
    drop(guard);
    buf_ref
}

/// Release a buffer reference.
///
/// Decrements the reference count under the cache spinlock. If it reaches zero
/// the buffer moves to the head of the LRU list and waiters are woken.
pub fn brelse(buf_ref: BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_mut().unwrap();
    buf.refcnt -= 1;
    let refcnt = buf.refcnt;
    if refcnt == 0 {
        cache.move_to_head(buf_ref.index());
    }
}

/// Write a buffer's contents to disk.
pub fn bwrite(buf_ref: &BufRef) {
    // Read identity under the cache spinlock, then release it before I/O.
    let (dev, blockno) = {
        let cache = BUF_CACHE.acquire();
        let buf = cache.buffers[buf_ref.index()].as_ref().unwrap();
        (buf.dev, buf.blockno)
    };
    let guard = buf_ref.lock();
    if guard.valid() {
        let data = guard.data().to_vec();
        drop(guard);
        write_block(dev, blockno, &data);
    }
}

/// Pin a buffer (increment refcount without using it).
///
/// Used by the log to keep blocks in cache during transactions.
pub fn bpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    cache.buffers[buf_ref.index()].as_mut().unwrap().refcnt += 1;
}

/// Unpin a buffer (decrement refcount).
pub fn bunpin(buf_ref: &BufRef) {
    let mut cache = BUF_CACHE.acquire();
    let buf = cache.buffers[buf_ref.index()].as_mut().unwrap();
    buf.refcnt -= 1;
    let refcnt = buf.refcnt;
    if refcnt == 0 {
        cache.move_to_head(buf_ref.index());
    }
}

/// Read a 1024-byte block from disk in one virtio request (2 sectors at once).
fn read_block(_dev: u32, blockno: u32, dst: &mut [u8]) {
    // A 1024-byte FS block = 2 consecutive 512-byte sectors starting at
    // blockno*2. One request moving 1024 bytes transfers both, halving the
    // virtio round-trips (lock + descriptor setup + notify + poll) vs a
    // per-sector loop.
    let mut block = crate::drivers::virtio::Block { blockno: (blockno as u64) * 2, data: [0u8; 1024] };
    virtio_rw(&mut block, false); // false = read
    dst[..1024].copy_from_slice(&block.data);
}

/// Write a 1024-byte block to disk in one virtio request (2 sectors at once).
fn write_block(_dev: u32, blockno: u32, src: &[u8]) {
    let mut block = crate::drivers::virtio::Block { blockno: (blockno as u64) * 2, data: [0u8; 1024] };
    block.data.copy_from_slice(&src[..1024]);
    virtio_rw(&mut block, true); // true = write
}
