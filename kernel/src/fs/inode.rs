// kernel/src/fs/inode.rs
//! Inode management and file system operations.
//!
//! Implements the inode layer: in-memory inodes, disk inode format,
//! directory operations, block allocation, and the inode cache.

use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::sync::sleeplock::SleepLock;
use crate::fs::buf::{bread, brelse, bwrite, BSIZE};
use crate::fs::log::{begin_op, end_op, SuperBlock};
use alloc::vec::Vec;
use core::str;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Number of direct block pointers in an inode.
pub const NDIRECT: usize = 12;

/// Number of indirect block pointers (one block of u32s).
pub const NINDIRECT: usize = BSIZE / 4;

/// Maximum file size in blocks (direct + indirect).
pub const MAXFILE: usize = NDIRECT + NINDIRECT;

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
    pub addrs: [u32; NDIRECT + 1],
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
            addrs: [0; NDIRECT + 1],
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
    pub addrs: [u32; NDIRECT + 1],
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
                addrs: [0; NDIRECT + 1],
            }, "inode_inner"),
            dev,
            inum,
            refcnt: AtomicUsize::new(0),
        }
    }
    
    /// Acquire the sleep lock (for I/O operations).
    pub fn lock(&self) -> crate::sync::sleeplock::SleepLockGuard<()> { 
        self.lock.acquire() 
    }
    
    /// Release the sleep lock.
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
    pub fn addrs(&self) -> [u32; NDIRECT + 1] {
        self.inner().addrs
    }
    
    /// Set the block addresses.
    pub fn set_addrs(&self, addrs: [u32; NDIRECT + 1]) {
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

    /// Map file block number `bn` to its disk block number.
    ///
    /// Handles the 12 direct blocks and the single indirect block. When `alloc`
    /// is set, missing data blocks (and the indirect block itself) are allocated
    /// with `balloc` and the indirect block is written back. Returns 0 when the
    /// block is unmapped and `alloc` is false, or on out-of-space / a `bn`
    /// beyond the maximum file size. Mirrors xv6's `bmap`.
    fn bmap(&self, inner: &mut InodeInner, bn: usize, alloc: bool) -> u32 {
        // Direct blocks.
        if bn < NDIRECT {
            let mut addr = inner.addrs[bn];
            if addr == 0 && alloc {
                addr = balloc(self.dev);
                inner.addrs[bn] = addr;
            }
            return addr;
        }

        // Indirect block: `bn - NDIRECT` indexes the block of pointers at
        // addrs[NDIRECT].
        let idx = bn - NDIRECT;
        if idx >= NINDIRECT {
            return 0; // beyond MAXFILE
        }

        let mut ind = inner.addrs[NDIRECT];
        if ind == 0 {
            if !alloc {
                return 0;
            }
            ind = balloc(self.dev);
            if ind == 0 {
                return 0;
            }
            inner.addrs[NDIRECT] = ind;
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
            bwrite(&bp);
        }
        brelse(bp);
        addr
    }

    /// Read data from the inode.
    ///
    /// # Arguments
    /// * `dst` - Destination buffer
    /// * `off` - Offset in file
    /// * `n` - Number of bytes to read
    ///
    /// Returns number of bytes read (0 at EOF).
    pub fn read(&self, dst: &mut [u8], off: usize, n: usize) -> usize {
        let _lock = self.lock();
        let mut inner = self.inner();
        let size = inner.size as usize;

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
            let disk_bn = self.bmap(&mut inner, bn, false);
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
    pub fn write(&self, src: &[u8], off: usize, n: usize) -> usize {
        let _lock = self.lock();
        let mut inner = self.inner();
        let size = inner.size as usize;
        
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

            // Resolve (allocating direct/indirect blocks as needed).
            let disk_bn = self.bmap(&mut inner, bn, true);
            if disk_bn == 0 {
                break; // Out of space
            }

            let bp = bread(self.dev, disk_bn);
            {
                let mut buf = bp.lock();
                let data = buf.data_mut();
                data[boff..boff + chunk].copy_from_slice(&src[total..total + chunk]);
            }
            bwrite(&bp);
            brelse(bp);
            
            total += chunk;
            offset += chunk;
            remaining -= chunk;
        }
        
        if offset > size {
            inner.size = offset as u32;
        }
        
        total
    }

    /// Truncate the inode to zero length.
    /// 
    /// Frees all data blocks and resets size to 0.
    pub fn truncate(&self) {
        let _lock = self.lock();
        let mut inner = self.inner();
        
        // Free direct blocks
        for i in 0..NDIRECT {
            if inner.addrs[i] != 0 {
                bfree(self.dev, inner.addrs[i]);
                inner.addrs[i] = 0;
            }
        }
        
        // Free indirect blocks
        if inner.addrs[NDIRECT] != 0 {
            let bp = bread(self.dev, inner.addrs[NDIRECT]);
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
            bfree(self.dev, inner.addrs[NDIRECT]);
            inner.addrs[NDIRECT] = 0;
        }
        
        inner.size = 0;
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
    
    // Find empty slot
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            // Check typ without holding guard across unsafe block
            let typ = {
                let inner = inode.inner();
                inner.typ
            };
            if typ == InodeType::None {
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    (*inode_ptr).dev = dev;
                    (*inode_ptr).inum = inum;
                    (*inode_ptr).refcnt.store(1, Ordering::Release);
                    // Use spinlock directly to set typ
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
                        inner.addrs = [0; NDIRECT + 1];
                    }
                    break;
                }
            }
        }
    }
}

pub fn ialloc(dev: u32, typ: InodeType) -> Option<&'static Inode> {
    begin_op();
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
            new_dip.addrs = [0; NDIRECT + 1];
            unsafe {
                *(buf.data_mut().as_mut_ptr().add(off(inum)) as *mut DiskInode) = new_dip;
            }
            drop(buf); // release the sleeplock before bwrite/brelse
            bwrite(&bp);
            brelse(bp);
            end_op();
            return Some(iget(dev, inum));
        }
        drop(buf);
        brelse(bp);
    }
    
    end_op();
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
    bwrite(&bp);
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

pub fn dirlink(dp: &Inode, name: &str, inum: u32) -> Result<(), &'static str> {
    dp.lock();
    
    if dp.typ() != InodeType::Dir {
        dp.unlock();
        return Err("not a directory");
    }
    
    // Check if name already exists
    if dirlookup_locked(dp, name).is_some() {
        dp.unlock();
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
            dp.unlock();
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
    dp.unlock();
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
        bwrite(&bp);
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
    bwrite(&bp);
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