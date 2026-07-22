// kernel/src/fs/inode.rs
use crate::mm::address::PhysAddr;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InodeType {
    Directory,
    File,
    Device,
}

#[derive(Clone)]
pub struct Inode {
    pub dev: u32,
    pub inum: u32,
    pub typ: InodeType,
    pub nlink: u32,
    pub size: usize,
    pub addrs: [PhysAddr; 12],
}

impl Inode {
    pub fn new(dev: u32, inum: u32, typ: InodeType) -> Self {
        Self {
            dev,
            inum,
            typ,
            nlink: 0,
            size: 0,
            addrs: [PhysAddr(0); 12],
        }
    }
}

pub fn namei(path: &str) -> Result<Inode, &'static str> {
    // Simple root directory lookup
    if path == "/" {
        Ok(Inode::new(1, 1, InodeType::Directory))
    } else {
        Err("not found")
    }
}

pub fn nameiparent(path: &str) -> Result<(Inode, &str), &'static str> {
    // Return parent directory and final name component
    if path == "/" {
        Err("no parent")
    } else {
        Ok((Inode::new(1, 1, InodeType::Directory), path))
    }
}

pub fn dirlink(_dp: &Inode, _name: &str, _inum: u32) -> Result<(), &'static str> {
    // Add directory entry
    Ok(())
}

pub fn dirlookup(_dp: &Inode, _name: &str) -> Result<Option<Inode>, &'static str> {
    // Lookup directory entry
    Ok(None)
}

pub fn ialloc(dev: u32, typ: InodeType) -> Inode {
    static mut NEXT_INUM: u32 = 2;
    unsafe {
        let inum = NEXT_INUM;
        NEXT_INUM += 1;
        Inode::new(dev, inum, typ)
    }
}

pub fn iupdate(_ip: &Inode) {
    // Update inode on disk
}

pub fn iput(_ip: Inode) {
    // Put inode back to cache
}

pub fn iunlockput(_ip: Inode) {
    // Unlock and put inode
}

pub fn iunlock(_ip: &Inode) {
    // Unlock inode - in Rust, we just drop the guard
}