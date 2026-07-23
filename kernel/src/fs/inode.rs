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
    
    /// Set the major device number.
    pub fn set_major(&self, major: u16) {
        self.inner().major = major;
    }
    
    /// Set the minor device number.
    pub fn set_minor(&self, minor: u16) {
        self.inner().minor = minor;
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
        let inner = self.inner();
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
            
            let bp = bread(self.dev, inner.addrs[bn]);
            let buf = bp.lock();
            let data = buf.data();
            dst[total..total + chunk].copy_from_slice(&data[boff..boff + chunk]);
            drop(buf);
            brelse(bp);
            
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
            
            // Allocate block if needed
            if inner.addrs[bn] == 0 {
                inner.addrs[bn] = balloc(self.dev);
                if inner.addrs[bn] == 0 {
                    break; // Out of space
                }
            }
            
            let bp = bread(self.dev, inner.addrs[bn]);
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
    crate::arch::console::printk(format_args!("iinit: start\n"));
    let mut cache = ICACHE.acquire();
    for i in 0..NINODE {
        cache.inodes[i] = Some(Inode::new(0, 0));
    }
    crate::arch::console::printk(format_args!("iinit: done\n"));
}

fn iget_locked(dev: u32, inum: u32) -> &'static Inode {
    crate::arch::console::printk(format_args!("iget_locked: start\n"));
    let mut cache = ICACHE.acquire();
    crate::arch::console::printk(format_args!("iget_locked: got cache, NINODE={}\n", NINODE));
    
    // Search for existing inode
    for i in 0..NINODE {
        crate::arch::console::printk(format_args!("iget_locked: checking slot {}\n", i));
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
        crate::arch::console::printk(format_args!("iget_locked: checking empty slot {}\n", i));
        if let Some(inode) = &cache.inodes[i] {
            // Check typ without holding guard across unsafe block
            let typ = {
                let inner = inode.inner();
                inner.typ
            };
            crate::arch::console::printk(format_args!("iget_locked: got inner, typ={:?}\n", typ));
            if typ == InodeType::None {
                crate::arch::console::printk(format_args!("iget_locked: typ is None, entering if\n"));
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    crate::arch::console::printk(format_args!("iget_locked: before dev assignment\n"));
                    (*inode_ptr).dev = dev;
                    crate::arch::console::printk(format_args!("iget_locked: after dev assignment\n"));
                    (*inode_ptr).inum = inum;
                    crate::arch::console::printk(format_args!("iget_locked: after inum assignment\n"));
                    (*inode_ptr).refcnt.store(1, Ordering::Release);
                    crate::arch::console::printk(format_args!("iget_locked: after refcnt store\n"));
                    // Use spinlock directly to set typ
                    (*inode_ptr).spinlock.acquire().typ = InodeType::None;
                    crate::arch::console::printk(format_args!("iget_locked: after inner typ assignment\n"));
                }
                crate::arch::console::printk(format_args!("iget_locked: found empty slot\n"));
                return unsafe { &*(inode as *const Inode) };
            }
        }
    }
    
    panic!("iget: no inodes available");
}

pub fn iget(dev: u32, inum: u32) -> &'static Inode {
    crate::arch::console::printk(format_args!("iget: dev={} inum={}\n", dev, inum));
    let ip = iget_locked(dev, inum);
    crate::arch::console::printk(format_args!("iget: iget_locked done\n"));
    
    // Load from disk if needed
    let needs_load = {
        let inner = ip.inner();
        inner.typ == InodeType::None
    };
    crate::arch::console::printk(format_args!("iget: needs_load={}\n", needs_load));
    
    if needs_load {
        let sb = read_superblock(dev);
        let bp = bread(dev, IBLOCK(inum, &sb));
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
    for inum in 1..sb.ninodes {
        let bp = bread(dev, IBLOCK(inum, &sb));
        {
            let buf = bp.lock();
            let data = buf.data();
            let dip: &DiskInode = unsafe {
                &*(data.as_ptr().add((inum as usize % IPB as usize) * core::mem::size_of::<DiskInode>()) as *const DiskInode)
            };
            if dip.typ == 0 {
                // Found free inode
                let mut new_dip = *dip;
                new_dip.typ = typ as u16;
                new_dip.nlink = 1;
                new_dip.size = 0;
                new_dip.addrs = [0; NDIRECT + 1];
                
                // Write back
                {
                    let mut buf = bp.lock();
                    let data = buf.data_mut();
                    let dip_mut: &mut DiskInode = unsafe {
                        &mut *(data.as_mut_ptr().add((inum as usize % IPB as usize) * core::mem::size_of::<DiskInode>()) as *mut DiskInode)
                    };
                    *dip_mut = new_dip;
                }
                bwrite(&bp);
                brelse(bp);
                
                end_op();
                return Some(iget(dev, inum));
            }
        }
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
pub fn namei(path: &str) -> Result<&'static Inode, &'static str> {
    crate::arch::console::printk(format_args!("namei: path={}\n", path));
    let (dp, name) = nameiparent(path)?;
    crate::arch::console::printk(format_args!("namei: got dp, name={}\n", name));
    let result = dirlookup(dp, name);
    crate::arch::console::printk(format_args!("namei: dirlookup done\n"));
    iput(dp);
    crate::arch::console::printk(format_args!("namei: done\n"));
    result
}

pub fn nameiparent(path: &str) -> Result<(&'static Inode, &str), &'static str> {
    crate::arch::console::printk(format_args!("nameiparent: path={}\n", path));
    let mut dp = iget(ROOTDEV, ROOTINO);
    crate::arch::console::printk(format_args!("nameiparent: iget done\n"));
    
    if path == "/" {
        return Err("no parent");
    }
    
    // Simple path parsing - split by '/'
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return Err("invalid path");
    }
    
    let name = components.last().unwrap();
    let parent_components = &components[..components.len() - 1];
    
    for comp in parent_components {
        dp.lock();
        let next = dirlookup(dp, comp)?;
        dp.unlock();
        iput(dp);
        dp = next;
    }
    
    Ok((dp, name))
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