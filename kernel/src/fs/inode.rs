// kernel/src/fs/inode.rs
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::sync::sleeplock::SleepLock;
use crate::fs::buf::{bread, brelse, bwrite, BSIZE};
use crate::fs::log::{begin_op, end_op, SuperBlock};
use alloc::vec::Vec;
use core::str;

pub const NDIRECT: usize = 12;
pub const NINDIRECT: usize = BSIZE / 4;
pub const MAXFILE: usize = NDIRECT + NINDIRECT;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct DiskInode {
    pub typ: u16,
    pub major: u16,
    pub minor: u16,
    pub nlink: u16,
    pub size: u32,
    pub addrs: [u32; NDIRECT + 1],
}

impl DiskInode {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InodeType {
    None = 0,
    Dir = 1,
    File = 2,
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

pub struct Inode {
    lock: SleepLock<()>,
    spinlock: SpinLock<InodeInner>,
    dev: u32,
    inum: u32,
    refcnt: usize,
}

struct InodeInner {
    typ: InodeType,
    major: u16,
    minor: u16,
    nlink: u16,
    size: u32,
    addrs: [u32; NDIRECT + 1],
}

impl Inode {
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
            refcnt: 0,
        }
    }
    
    pub fn lock(&self) -> crate::sync::sleeplock::SleepLockGuard<()> { 
        self.lock.acquire() 
    }
    
    pub fn unlock(&self) {
        self.lock.release();
    }
    
    pub fn dev(&self) -> u32 { self.dev }
    pub fn inum(&self) -> u32 { self.inum }
    pub fn refcnt(&self) -> usize { self.refcnt }
    
    fn inner(&self) -> SpinLockGuard<InodeInner> {
        self.spinlock.acquire()
    }
    
    pub fn typ(&self) -> InodeType {
        self.inner().typ
    }
    
    pub fn size(&self) -> u32 {
        self.inner().size
    }
    
    pub fn nlink(&self) -> u16 {
        self.inner().nlink
    }
    
    pub fn set_nlink(&self, n: u16) {
        self.inner().nlink = n;
    }
    
    pub fn inc_nlink(&self) {
        self.inner().nlink += 1;
    }
    
    pub fn dec_nlink(&self) {
        self.inner().nlink -= 1;
    }
    
    pub fn set_size(&self, size: u32) {
        self.inner().size = size;
    }
    
    pub fn addrs(&self) -> [u32; NDIRECT + 1] {
        self.inner().addrs
    }
    
    pub fn set_addrs(&self, addrs: [u32; NDIRECT + 1]) {
        self.inner().addrs = addrs;
    }
    
    pub fn set_type(&self, typ: InodeType) {
        self.inner().typ = typ;
    }
    
    pub fn set_major(&self, major: u16) {
        self.inner().major = major;
    }
    
    pub fn set_minor(&self, minor: u16) {
        self.inner().minor = minor;
    }

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
                // Need to modify refcnt - use interior mutability or unsafe
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    (*inode_ptr).refcnt += 1;
                }
                return unsafe { &*(inode as *const Inode) };
            }
        }
    }
    
    // Find empty slot
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            let inner = inode.inner();
            if inner.typ == InodeType::None {
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    (*inode_ptr).dev = dev;
                    (*inode_ptr).inum = inum;
                    (*inode_ptr).refcnt = 1;
                    (*inode_ptr).inner().typ = InodeType::None; // Will be loaded from disk
                }
                return unsafe { &*(inode as *const Inode) };
            }
        }
    }
    
    panic!("iget: no inodes available");
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
    for i in 0..NINODE {
        if let Some(inode) = &cache.inodes[i] {
            if core::ptr::eq(inode, ip) {
                unsafe {
                    let inode_ptr = inode as *const Inode as *mut Inode;
                    (*inode_ptr).refcnt -= 1;
                    if (*inode_ptr).refcnt == 0 {
                        let inner = (*inode_ptr).inner();
                        if inner.nlink == 0 {
                            // Truncate and free inode
                            drop(cache);
                            ip.truncate();
                            let mut cache = ICACHE.acquire();
                            let mut inner = (*inode_ptr).inner();
                            inner.typ = InodeType::None;
                            inner.size = 0;
                            inner.addrs = [0; NDIRECT + 1];
                        }
                    }
                }
                break;
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
    let (dp, name) = nameiparent(path)?;
    let result = dirlookup(dp, name);
    iput(dp);
    result
}

pub fn nameiparent(path: &str) -> Result<(&'static Inode, &str), &'static str> {
    let mut dp = iget(ROOTDEV, ROOTINO);
    
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

fn dirlookup_locked(dp: &Inode, name: &str) -> Option<&'static Inode> {
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
            let buf = bp.lock();
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
const DIRSIZ: usize = 14;

#[repr(C)]
#[derive(Copy, Clone)]
struct Dirent {
    inum: u16,
    name: [u8; DIRSIZ],
}

impl Dirent {
    fn new() -> Self {
        Self { inum: 0, name: [0; DIRSIZ] }
    }
    
    fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, core::mem::size_of::<Self>()) }
    }
    
    fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut Self as *mut u8, core::mem::size_of::<Self>()) }
    }
}