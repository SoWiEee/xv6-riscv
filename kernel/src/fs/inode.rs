// kernel/src/fs/inode.rs
//! Inode management and file system operations.
//!
//! Implements the inode layer: in-memory inodes, disk inode format,
//! directory operations, block allocation, and the inode cache.

use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::sync::sleeplock::SleepLock;
use crate::fs::buf::{bread, brelse, BSIZE};
use crate::fs::log::{log_write, SuperBlock};
use alloc::vec::Vec;
use core::str;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Number of direct block pointers in an inode. One slot was traded from the
/// original 12 to make room for the doubly-indirect pointer while keeping the
/// on-disk inode at 13 addr slots (64 bytes). MUST match `NDIRECT` in the C
/// `kernel/fs.h` used by mkfs, or packed files are misread.
pub const NDIRECT: usize = 11;

/// Number of block pointers in one indirect block (a full block of u32s).
pub const NINDIRECT: usize = BSIZE / 4;

/// Maximum file size in blocks: direct + one singly-indirect + one
/// doubly-indirect level (11 + 256 + 256*256 ≈ 64 MiB).
pub const MAXFILE: usize = NDIRECT + NINDIRECT + NINDIRECT * NINDIRECT;

/// Total inode address slots: NDIRECT direct + 1 singly-indirect (`addrs[NDIRECT]`)
/// + 1 doubly-indirect (`addrs[NDIRECT + 1]`). Stays 13 to keep the 64-byte
/// on-disk inode layout unchanged.
pub const NADDR: usize = NDIRECT + 2;

/// On-disk inode structure.
/// 
/// Matches the C xv6 layout exactly for disk compatibility.
/// 64 bytes total.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct DiskInode {
    /// File type (0=free, 1=dir, 2=file, 3=device)
    pub typ: u16,
    /// Major device number (for device files)
    pub major: u16,
    /// Minor device number (for device files)
    pub minor: u16,
    /// Number of directory links
    pub nlink: u16,
    /// File size in bytes
    pub size: u32,
    /// Block addresses (12 direct + 1 indirect)
    pub addrs: [u32; NADDR],
}

impl DiskInode {
    /// Create a zeroed disk inode.
    pub fn new() -> Self {
        Self {
            typ: 0,
            major: 0,
            minor: 0,
            nlink: 0,
            size: 0,
            addrs: [0; NADDR],
        }
    }
}

/// In-memory inode type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InodeType {
    /// Unallocated
    None = 0,
    /// Directory
    Dir = 1,
    /// Regular file
    File = 2,
    /// Device file
    Device = 3,
}

impl From<u16> for InodeType {
    fn from(val: u16) -> Self {
        match val {
            0 => InodeType::None,
            1 => InodeType::Dir,
            2 => InodeType::File,
            3 => InodeType::Device,
            _ => InodeType::None,
        }
    }
}

/// In-memory inode.
/// 
/// Uses two locks:
/// - `lock` (SleepLock): For operations that may sleep (I/O)
/// - `spinlock` (SpinLock): For quick metadata access
pub struct Inode {
    lock: SleepLock<()>,
    spinlock: SpinLock<InodeInner>,
    dev: u32,
    inum: u32,
    refcnt: AtomicUsize,
}

/// Mutable inode metadata protected by `spinlock`.
pub struct InodeInner {
    pub typ: InodeType,
    pub major: u16,
    pub minor: u16,
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NADDR],
}

impl Inode {
    /// Create a new uninitialized inode.
    pub fn new(dev: u32, inum: u32) -> Self {
        Self {
            lock: SleepLock::new((), "inode"),
            spinlock: SpinLock::new(InodeInner {
                typ: InodeType::None,
                major: 0,
                minor: 0,
                nlink: 0,
                size: 0,
                addrs: [0; NADDR],
            }, "inode_inner"),
            dev,
            inum,
            refcnt: AtomicUsize::new(0),
        }
    }
    
    /// Acquire the inode's sleep lock (xv6 `ilock`), holding it until `unlock`.
    ///
    /// The RAII guard is `forget`-ten so the lock stays held across the caller's
    /// whole operation — `read`/`write`/`truncate`/`dirlink`/`dirlookup_locked`
    /// all assume the caller already holds this lock and do NOT re-acquire it
    /// (the sleeplock is not reentrant). Every `lock()` must be balanced by
    /// exactly one `unlock()`.
    pub fn lock(&self) {
        core::mem::forget(self.lock.acquire());
    }

    /// Release the inode's sleep lock (xv6 `iunlock`).
    pub fn unlock(&self) {
        self.lock.release();
    }
    
    /// Get the device number.
    pub fn dev(&self) -> u32 { self.dev }
    
    /// Get the inode number.
    pub fn inum(&self) -> u32 { self.inum }
    
    /// Get the reference count.
    pub fn refcnt(&self) -> usize { self.refcnt.load(Ordering::Acquire) }
    
    /// Acquire the spinlock for metadata access.
    pub fn inner(&self) -> SpinLockGuard<InodeInner> {
        self.spinlock.acquire()
    }
    
    /// Get the inode type.
    pub fn typ(&self) -> InodeType {
        self.inner().typ
    }
    
    /// Get the file size.
    pub fn size(&self) -> u32 {
        self.inner().size
    }
    
    /// Get the link count.
    pub fn nlink(&self) -> u16 {
        self.inner().nlink
    }
    
    /// Set the link count.
    pub fn set_nlink(&self, n: u16) {
        self.inner().nlink = n;
    }
    
    /// Increment the link count.
    pub fn inc_nlink(&self) {
        self.inner().nlink += 1;
    }
    
    /// Decrement the link count.
    pub fn dec_nlink(&self) {
        self.inner().nlink -= 1;
    }
    
    /// Set the file size.
    pub fn set_size(&self, size: u32) {
        self.inner().size = size;
    }
    
    /// Get the block addresses.
    pub fn addrs(&self) -> [u32; NADDR] {
        self.inner().addrs
    }
    
    /// Set the block addresses.
    pub fn set_addrs(&self, addrs: [u32; NADDR]) {
        self.inner().addrs = addrs;
    }
    
    /// Set the inode type.
    pub fn set_type(&self, typ: InodeType) {
        self.inner().typ = typ;
    }
    
    /// Get the major device number.
    pub fn major(&self) -> u16 {
        self.inner().major
    }

    /// Set the major device number.
    pub fn set_major(&self, major: u16) {
        self.inner().major = major;
    }
    
    /// Set the minor device number.
    pub fn set_minor(&self, minor: u16) {
        self.inner().minor = minor;
    }

    /// Fetch (allocating when `alloc`) the block number stored at entry `idx` of
    /// the indirect block whose own block number lives in `*slot`. The indirect
    /// block itself is allocated on demand (and `*slot` updated). Returns 0 when
    /// unallocated and `!alloc`, or on out of space. One level of the walk shared
    /// by the singly- and doubly-indirect cases.
    fn indirect_entry(&self, slot: &mut u32, idx: usize, alloc: bool) -> u32 {
        let mut ind = *slot;
        if ind == 0 {
            if !alloc {
                return 0;
            }
            ind = balloc(self.dev);
            if ind == 0 {
                return 0;
            }
            *slot = ind;
        }

        let bp = bread(self.dev, ind);
        let addr;
        let mut dirty = false;
        {
            let mut buf = bp.lock();
            let data = buf.data_mut();
            // SAFETY: a block is BSIZE bytes = NINDIRECT u32 entries and the
            // buffer is suitably aligned for u32 access.
            let entries = unsafe {
                core::slice::from_raw_parts_mut(data.as_mut_ptr() as *mut u32, NINDIRECT)
            };
            let mut a = entries[idx];
            if a == 0 && alloc {
                a = balloc(self.dev);
                entries[idx] = a;
                dirty = a != 0;
            }
            addr = a;
        }
        if dirty {
            log_write(&bp);
        }
        brelse(bp);
        addr
    }

    /// Map file block number `bn` to its disk block number.
    ///
    /// Handles the direct blocks, the singly-indirect block (`addrs[NDIRECT]`),
    /// and the doubly-indirect block (`addrs[NDIRECT + 1]`). When `alloc` is set,
    /// missing data blocks and the indirect blocks themselves are allocated.
    /// Returns 0 when unmapped and `!alloc`, on out of space, or for a `bn`
    /// beyond MAXFILE. Mirrors xv6's `bmap`.
    /// Operates on a caller-owned copy of the inode's `addrs` (never the
    /// InodeInner spinlock): bmap does buffer I/O whose sleeplock release wakes
    /// processes and takes proc locks, so it MUST NOT run under a spinlock. The
    /// inode sleeplock the caller holds keeps `addrs` stable meanwhile; `write`/
    /// `truncate` copy any bmap-allocated pointers back afterward.
    fn bmap(&self, addrs: &mut [u32; NADDR], bn: usize, alloc: bool) -> u32 {
        // Direct blocks.
        if bn < NDIRECT {
            let mut addr = addrs[bn];
            if addr == 0 && alloc {
                addr = balloc(self.dev);
                addrs[bn] = addr;
            }
            return addr;
        }
        let bn = bn - NDIRECT;

        // Singly-indirect: one block of NINDIRECT data-block pointers.
        if bn < NINDIRECT {
            return self.indirect_entry(&mut addrs[NDIRECT], bn, alloc);
        }
        let bn = bn - NINDIRECT;

        // Doubly-indirect: a block of NINDIRECT pointers, each to a block of
        // NINDIRECT data-block pointers.
        if bn < NINDIRECT * NINDIRECT {
            // First level index selects the second-level block; second index
            // selects the data block within it.
            let l1_block = self.indirect_entry(&mut addrs[NDIRECT + 1], bn / NINDIRECT, alloc);
            if l1_block == 0 {
                return 0;
            }
            // `l1_block` is already allocated, so indirect_entry won't rewrite
            // this local slot; it just walks into the second-level block.
            let mut second = l1_block;
            return self.indirect_entry(&mut second, bn % NINDIRECT, alloc);
        }

        0 // beyond MAXFILE
    }

    /// Read data from the inode.
    ///
    /// # Arguments
    /// * `dst` - Destination buffer
    /// * `off` - Offset in file
    /// * `n` - Number of bytes to read
    ///
    /// Returns number of bytes read (0 at EOF).
    /// Read from the inode. The caller MUST hold the inode lock (`lock()`).
    pub fn read(&self, dst: &mut [u8], off: usize, n: usize) -> usize {
        // Snapshot size + addrs, then release the InodeInner spinlock: the
        // caller's inode sleeplock keeps them stable, and bmap/bread below must
        // not run under a spinlock (their buffer sleeplock release wakes procs).
        let (size, mut addrs) = {
            let inner = self.inner();
            (inner.size as usize, inner.addrs)
        };

        if off >= size {
            return 0;
        }

        let n = core::cmp::min(n, size - off);
        let mut total = 0;
        let mut offset = off;
        let mut remaining = n;

        while remaining > 0 {
            let bn = offset / BSIZE;
            let boff = offset % BSIZE;
            let chunk = core::cmp::min(remaining, BSIZE - boff);

            // Resolve the logical block to a disk block (direct or indirect).
            let disk_bn = self.bmap(&mut addrs, bn, false);
            if disk_bn == 0 {
                // Sparse hole within the file: reads as zeros.
                for b in dst[total..total + chunk].iter_mut() {
                    *b = 0;
                }
            } else {
                let bp = bread(self.dev, disk_bn);
                let buf = bp.lock();
                let data = buf.data();
                dst[total..total + chunk].copy_from_slice(&data[boff..boff + chunk]);
                drop(buf);
                brelse(bp);
            }

            total += chunk;
            offset += chunk;
            remaining -= chunk;
        }

        total
    }

    /// Write data to the inode.
    /// 
    /// # Arguments
    /// * `src` - Source buffer
    /// * `off` - Offset in file
    /// * `n` - Number of bytes to write
    /// 
    /// Returns number of bytes written (may be less than `n` if disk full).
    /// Allocates blocks as needed.
    /// Write to the inode. The caller MUST hold the inode lock (`lock()`) and
    /// run inside a log transaction (`begin_op`/`end_op`).
    pub fn write(&self, src: &[u8], off: usize, n: usize) -> usize {
        // Snapshot size + addrs and drop the spinlock (see `read`); bmap may
        // allocate blocks, updating our local `addrs`, which we write back below.
        let (mut size, mut addrs) = {
            let inner = self.inner();
            (inner.size as usize, inner.addrs)
        };

        if off > size {
            return 0;
        }

        let mut total = 0;
        let mut offset = off;
        let mut remaining = n;

        while remaining > 0 {
            let bn = offset / BSIZE;
            let boff = offset % BSIZE;
            let chunk = core::cmp::min(remaining, BSIZE - boff);

            if bn >= MAXFILE {
                break;
            }

            // Resolve (allocating direct/indirect blocks as needed) on the local
            // addrs copy — no spinlock held across the buffer I/O.
            let disk_bn = self.bmap(&mut addrs, bn, true);
            if disk_bn == 0 {
                break; // Out of space
            }

            let bp = bread(self.dev, disk_bn);
            {
                let mut buf = bp.lock();
                let data = buf.data_mut();
                data[boff..boff + chunk].copy_from_slice(&src[total..total + chunk]);
            }
            log_write(&bp);
            brelse(bp);

            total += chunk;
            offset += chunk;
            remaining -= chunk;
        }

        if offset > size {
            size = offset;
        }

        // Publish the (possibly grown) size and any blocks bmap allocated back
        // into the in-memory inode under a brief spinlock, then persist. Without
        // this the grown file's metadata would be lost on reboot.
        {
            let mut inner = self.inner();
            inner.addrs = addrs;
            inner.size = size as u32;
        }
        iupdate(self);

        total
    }

    /// Truncate the inode to zero length, freeing all data blocks. The caller
    /// MUST hold the inode lock (`lock()`) and run inside a log transaction.
    pub fn truncate(&self) {
        // Snapshot addrs and drop the spinlock (see `read`): all the bfree/bread
        // below do buffer I/O whose sleeplock release takes proc locks and must
        // not run under the InodeInner spinlock. The caller's sleeplock keeps the
        // block map stable; we zero it and write it back at the end.
        let mut addrs = {
            let inner = self.inner();
            inner.addrs
        };

        // Free direct blocks
        for i in 0..NDIRECT {
            if addrs[i] != 0 {
                bfree(self.dev, addrs[i]);
                addrs[i] = 0;
            }
        }

        // Free indirect blocks
        if addrs[NDIRECT] != 0 {
            let bp = bread(self.dev, addrs[NDIRECT]);
            {
                let buf = bp.lock();
                let data = buf.data();
                // Indirect block contains array of u32 block numbers
                let indirect: &[u32] = unsafe {
                    core::slice::from_raw_parts(data.as_ptr() as *const u32, NINDIRECT)
                };
                for &bno in indirect {
                    if bno != 0 {
                        bfree(self.dev, bno);
                    }
                }
            }
            brelse(bp);
            bfree(self.dev, addrs[NDIRECT]);
            addrs[NDIRECT] = 0;
        }

        // Free the doubly-indirect tree: for each first-level entry, free every
        // data block in its second-level block plus that block, then the
        // first-level block itself. Copy the first-level pointers out before
        // freeing so we don't hold its buffer across the inner bfrees.
        if addrs[NDIRECT + 1] != 0 {
            let bp = bread(self.dev, addrs[NDIRECT + 1]);
            let l1: [u32; NINDIRECT] = {
                let buf = bp.lock();
                let data = buf.data();
                let src = unsafe {
                    core::slice::from_raw_parts(data.as_ptr() as *const u32, NINDIRECT)
                };
                let mut arr = [0u32; NINDIRECT];
                arr.copy_from_slice(src);
                arr
            };
            brelse(bp);

            for &l2block in l1.iter() {
                if l2block == 0 {
                    continue;
                }
                let bp2 = bread(self.dev, l2block);
                {
                    let buf = bp2.lock();
                    let data = buf.data();
                    let entries = unsafe {
                        core::slice::from_raw_parts(data.as_ptr() as *const u32, NINDIRECT)
                    };
                    for &bno in entries {
                        if bno != 0 {
                            bfree(self.dev, bno);
                        }
                    }
                }
                brelse(bp2);
                bfree(self.dev, l2block);
            }
            bfree(self.dev, addrs[NDIRECT + 1]);
            addrs[NDIRECT + 1] = 0;
        }

        // Publish the cleared block map + zero size, then persist so the on-disk
        // inode stops pointing at the freed blocks.
        {
            let mut inner = self.inner();
            inner.addrs = addrs;
            inner.size = 0;
        }
        iupdate(self);
    }
}

// Inode cache
const NINODE: usize = 50;

struct ICache {
    inodes: [Option<Inode>; NINODE],
}

static ICACHE: SpinLock<ICache> = SpinLock::new(ICache::new(), "icache");

impl ICache {
    const fn new() -> Self {
        Self { inodes: [const { None }; NINODE] }
    }
}

pub fn iinit() {
    let mut cache = ICACHE.acquire();
    for i in 0..NINODE {
        cache.inodes[i] = Some(Inode::new(0, 0));
    }
}

fn iget_locked(dev: u32, inum: u32) -> &'static Inode {
    let mut cache = ICACHE.acquire();

    // Search for existing inode
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            let inner = inode.inner();
            if inner.typ != InodeType::None && inode.dev == dev && inode.inum == inum {
                inode.refcnt.fetch_add(1, Ordering::AcqRel);
                return unsafe { &*(inode as *const Inode) };
            }
        }
    }
    
    // Recycle the first slot with no in-memory references. xv6 reuses any
    // ref==0 inode here, NOT only typ==None ones: a still-linked file that was
    // opened and closed keeps `typ` set but drops to refcnt 0 (iput only clears
    // typ when nlink==0). Reusing only typ==None slots leaked one itable entry
    // per distinct file ever touched, eventually panicking here (e.g. grind).
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            if inode.refcnt.load(Ordering::Acquire) == 0 {
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    (*inode_ptr).dev = dev;
                    (*inode_ptr).inum = inum;
                    (*inode_ptr).refcnt.store(1, Ordering::Release);
                    // Clear typ so iget reloads this (possibly different) inode
                    // from disk instead of trusting the recycled slot's data.
                    (*inode_ptr).spinlock.acquire().typ = InodeType::None;
                }
                return unsafe { &*(inode as *const Inode) };
            }
        }
    }

    panic!("iget: no inodes available");
}

/// Increment an inode's in-memory reference count and return a fresh handle,
/// mirroring xv6 `idup`. Used when a cached inode (e.g. a process cwd) becomes
/// the starting point of a path walk.
pub fn idup(ip: &Inode) -> &'static Inode {
    ip.refcnt.fetch_add(1, Ordering::AcqRel);
    unsafe { &*(ip as *const Inode) }
}

pub fn iget(dev: u32, inum: u32) -> &'static Inode {
    let ip = iget_locked(dev, inum);

    // Load from disk if needed
    let needs_load = {
        let inner = ip.inner();
        inner.typ == InodeType::None
    };

    if needs_load {
        let sb = read_superblock(dev);
        let iblock = IBLOCK(inum, &sb);
        let bp = bread(dev, iblock);
        {
            let buf = bp.lock();
            let data = buf.data();
            let dip: &DiskInode = unsafe {
                &*(data.as_ptr().add((inum as usize % IPB as usize) * core::mem::size_of::<DiskInode>()) as *const DiskInode)
            };
            let mut inner = ip.inner();
            inner.typ = InodeType::from(dip.typ);
            inner.major = dip.major;
            inner.minor = dip.minor;
            inner.nlink = dip.nlink;
            inner.size = dip.size;
            inner.addrs = dip.addrs;
        }
        brelse(bp);
    }
    
    ip
}

pub fn iput(ip: &Inode) {
    let mut cache = ICACHE.acquire();
    
    // Find and decrement refcnt
    let mut should_truncate = false;
    
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            if core::ptr::eq(inode, ip) {
                let prev = inode.refcnt.fetch_sub(1, Ordering::AcqRel);
                if prev == 1 {
                    // refcnt was 1, now 0
                    let inner = inode.inner();
                    if inner.nlink == 0 {
                        should_truncate = true;
                    }
                }
                break;
            }
        }
    }
    drop(cache);
    
    if should_truncate {
        // Hold the inode lock while we free its blocks and clear it on disk, as
        // xv6 iput does. Caller is already inside a log transaction.
        ip.lock();
        ip.truncate();
        let mut cache = ICACHE.acquire();
        for i in 0..NINODE {
            if let Some(inode) = &cache.inodes[i] {
                if core::ptr::eq(inode, ip) {
                    unsafe {
                        let inode_ptr = inode as *const Inode as *mut Inode;
                        let mut inner = (*inode_ptr).inner();
                        inner.typ = InodeType::None;
                        inner.size = 0;
                        inner.addrs = [0; NADDR];
                    }
                    break;
                }
            }
        }
        ip.unlock();
    }
}

/// Allocate a fresh on-disk inode of `typ`. MUST be called inside a transaction
/// (begin_op/end_op); the caller opens it so the inode allocation, the directory
/// link, and any writes commit atomically.
pub fn ialloc(dev: u32, typ: InodeType) -> Option<&'static Inode> {
    let sb = read_superblock(dev);

    // Search for free inode
    let off = |inum: u32| (inum as usize % IPB as usize) * core::mem::size_of::<DiskInode>();
    for inum in 1..sb.ninodes {
        let bp = bread(dev, IBLOCK(inum, &sb));
        // Lock the inode block exactly once. The free check and the write-back
        // must share a single guard: re-locking `bp` while the first guard is
        // still held would try to acquire the same (non-reentrant) sleeplock a
        // second time and deadlock.
        let mut buf = bp.lock();
        let dip: DiskInode = unsafe {
            *(buf.data().as_ptr().add(off(inum)) as *const DiskInode)
        };
        if dip.typ == 0 {
            let mut new_dip = dip;
            new_dip.typ = typ as u16;
            new_dip.nlink = 1;
            new_dip.size = 0;
            new_dip.addrs = [0; NADDR];
            unsafe {
                *(buf.data_mut().as_mut_ptr().add(off(inum)) as *mut DiskInode) = new_dip;
            }
            drop(buf); // release the sleeplock before log_write/brelse
            log_write(&bp);
            brelse(bp);
            return Some(iget(dev, inum));
        }
        drop(buf);
        brelse(bp);
    }

    None
}

pub fn iupdate(ip: &Inode) {
    let sb = read_superblock(ip.dev());
    let bp = bread(ip.dev(), IBLOCK(ip.inum(), &sb));
    {
        let mut buf = bp.lock();
        let data = buf.data_mut();
        let dip: &mut DiskInode = unsafe {
            &mut *(data.as_mut_ptr().add((ip.inum() as usize % IPB as usize) * core::mem::size_of::<DiskInode>()) as *mut DiskInode)
        };
        let inner = ip.inner();
        dip.typ = inner.typ as u16;
        dip.major = inner.major;
        dip.minor = inner.minor;
        dip.nlink = inner.nlink;
        dip.size = inner.size;
        dip.addrs = inner.addrs;
    }
    log_write(&bp);
    brelse(bp);
}

// Directory operations
/// Resolve `path` to an inode, mirroring xv6 `namex`.
///
/// The walk starts at the filesystem root for an absolute path (leading `/`)
/// and at the calling process's current working directory otherwise. With
/// `want_parent` set, it stops one component early and returns the parent
/// directory together with the final path element (borrowed from `path`);
/// this backs `nameiparent` for create/link/unlink.
fn namex(path: &str, want_parent: bool) -> Result<(&'static Inode, &str), &'static str> {
    let mut ip: &'static Inode = if path.starts_with('/') {
        iget(ROOTDEV, ROOTINO)
    } else {
        // Start from the process cwd; fall back to root before it is set.
        let cwd_ptr = {
            let p = crate::proc::current_process();
            let inner = p.lock();
            inner.cwd
        };
        match cwd_ptr {
            Some(ptr) => idup(unsafe { &*ptr }),
            None => iget(ROOTDEV, ROOTINO),
        }
    };

    let components: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // "/" or "" has no path elements.
    if components.is_empty() {
        if want_parent {
            iput(ip);
            return Err("no parent");
        }
        return Ok((ip, ""));
    }

    let last = components.len() - 1;
    for (idx, comp) in components.iter().enumerate() {
        ip.lock();
        if ip.typ() != InodeType::Dir {
            ip.unlock();
            iput(ip);
            return Err("not a directory");
        }
        if want_parent && idx == last {
            // Stop before the final element; return the parent directory.
            ip.unlock();
            return Ok((ip, *comp));
        }
        let next = match dirlookup_locked(ip, comp) {
            Some(next) => next,
            None => {
                ip.unlock();
                iput(ip);
                return Err("not found");
            }
        };
        ip.unlock();
        iput(ip);
        ip = next;
    }

    if want_parent {
        iput(ip);
        return Err("no parent");
    }
    Ok((ip, ""))
}

pub fn namei(path: &str) -> Result<&'static Inode, &'static str> {
    namex(path, false).map(|(ip, _)| ip)
}

pub fn nameiparent(path: &str) -> Result<(&'static Inode, &str), &'static str> {
    namex(path, true)
}

/// Write a new directory entry (`name` -> `inum`) into `dp`. The caller MUST
/// already hold `dp`'s inode lock (all callers hold the parent directory locked)
/// and be inside a log transaction.
pub fn dirlink(dp: &Inode, name: &str, inum: u32) -> Result<(), &'static str> {
    if dp.typ() != InodeType::Dir {
        return Err("not a directory");
    }

    // Check if name already exists
    if dirlookup_locked(dp, name).is_some() {
        return Err("name exists");
    }

    // Find free slot in directory
    let mut offset = 0;
    let entry_size = core::mem::size_of::<Dirent>();
    
    while offset < dp.size() as usize {
        let mut de = Dirent::new();
        let n = dp.read(&mut de.as_bytes_mut()[..entry_size], offset, entry_size);
        if n != entry_size {
            break;
        }
        
        if de.inum == 0 {
            // Found free slot
            de.inum = inum as u16;
            let name_bytes = name.as_bytes();
            let copy_len = core::cmp::min(name_bytes.len(), DIRSIZ - 1);
            de.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            if copy_len < DIRSIZ {
                de.name[copy_len] = 0;
            }
            
            dp.write(&de.as_bytes()[..entry_size], offset, entry_size);
            return Ok(());
        }
        offset += entry_size;
    }
    
    // Append new entry
    let mut de = Dirent::new();
    de.inum = inum as u16;
    let name_bytes = name.as_bytes();
    let copy_len = core::cmp::min(name_bytes.len(), DIRSIZ - 1);
    de.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    if copy_len < DIRSIZ {
        de.name[copy_len] = 0;
    }
    
    dp.write(&de.as_bytes()[..entry_size], offset, entry_size);
    Ok(())
}

pub fn dirlookup(dp: &Inode, name: &str) -> Result<&'static Inode, &'static str> {
    dp.lock();
    let result = dirlookup_locked(dp, name);
    dp.unlock();
    result.ok_or("not found")
}

pub fn dirlookup_locked(dp: &Inode, name: &str) -> Option<&'static Inode> {
    if dp.typ() != InodeType::Dir {
        return None;
    }

    let entry_size = core::mem::size_of::<Dirent>();
    let mut offset = 0;

    while offset < dp.size() as usize {
        let mut de = Dirent::new();
        let n = dp.read(&mut de.as_bytes_mut()[..entry_size], offset, entry_size);
        if n != entry_size {
            break;
        }

        if de.inum != 0 {
            let entry_name = str::from_utf8(&de.name).ok()?.trim_end_matches('\0');
            if entry_name == name {
                return Some(iget(dp.dev(), de.inum as u32));
            }
        }
        offset += entry_size;
    }

    None
}

// Block allocation
const ROOTDEV: u32 = 1;
const ROOTINO: u32 = 1;
const FSSIZE: u32 = 2000;
const BPB: u32 = BSIZE as u32 * 8;
const IPB: u32 = BSIZE as u32 / core::mem::size_of::<DiskInode>() as u32;

fn IBLOCK(inum: u32, sb: &SuperBlock) -> u32 {
    inum / IPB + sb.inodestart
}

fn BBLOCK(bno: u32, sb: &SuperBlock) -> u32 {
    bno / BPB + sb.bmapstart
}

fn read_superblock(dev: u32) -> SuperBlock {
    let bp = bread(dev, 1); // Superblock is at block 1
    let buf = bp.lock();
    let data = buf.data();
    let sb: &SuperBlock = unsafe { &*(data.as_ptr() as *const SuperBlock) };
    let result = *sb;
    drop(buf);
    brelse(bp);
    result
}

fn balloc(dev: u32) -> u32 {
    let sb = read_superblock(dev);
    
    for b in 0..sb.nblocks {
        let bp = bread(dev, BBLOCK(b, &sb));
        {
            let mut buf = bp.lock();
            let data = buf.data();
            let byte = data[(b % BPB) as usize / 8];
            let bit = (b % BPB) % 8;
            if (byte & (1 << bit)) == 0 {
                // Free block found
                let mut data = buf.data_mut();
                data[(b % BPB) as usize / 8] |= 1 << bit;
            } else {
                drop(buf);
                brelse(bp);
                continue;
            }
        }
        log_write(&bp);
        brelse(bp);
        return b;
    }
    
    0 // No free blocks
}

fn bfree(dev: u32, bno: u32) {
    let sb = read_superblock(dev);
    let bp = bread(dev, BBLOCK(bno, &sb));
    {
        let mut buf = bp.lock();
        let data = buf.data_mut();
        data[(bno % BPB) as usize / 8] &= !(1 << (bno % BPB) % 8);
    }
    log_write(&bp);
    brelse(bp);
}

// Directory entry
pub const DIRSIZ: usize = 14;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Dirent {
    pub inum: u16,
    pub name: [u8; DIRSIZ],
}

impl Dirent {
    pub fn new() -> Self {
        Self { inum: 0, name: [0; DIRSIZ] }
    }
    
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, core::mem::size_of::<Self>()) }
    }
    
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut Self as *mut u8, core::mem::size_of::<Self>()) }
    }
}