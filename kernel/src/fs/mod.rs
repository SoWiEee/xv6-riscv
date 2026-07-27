// kernel/src/fs/mod.rs
pub mod buf;
pub mod inode;
pub mod log;
pub mod file;
pub mod pipe;

pub use buf::{BSIZE, bread, brelse, bwrite, bpin, bunpin, binit, BufRef, BUF_CACHE, BufGuard};
pub use inode::{
    Inode, InodeType, DiskInode, 
    namei, nameiparent, dirlink, dirlookup, dirlookup_locked, 
    ialloc, iget, iput, idup, iupdate,
    iinit, NDIRECT, NINDIRECT, MAXFILE,
    Dirent
};
pub use log::{initlog, begin_op, end_op, log_write, MAXOPBLOCKS, SuperBlock, recover_from_log};
pub use file::{File, FileType, filealloc, fileclose, filedup, fileread, filewrite, filestat, fileinit};
pub use pipe::Pipe;

pub fn fsinit() {
    binit();
    iinit();
    // Initialize superblock and log
    let sb = read_superblock(ROOTDEV);
    initlog(ROOTDEV, &sb);
    recover_from_log();
}

fn read_superblock(dev: u32) -> SuperBlock {
    let bp = bread(dev, 1);
    let buf = bp.lock();
    let data = buf.data();
    let sb: &SuperBlock = unsafe { &*(data.as_ptr() as *const SuperBlock) };
    let result = *sb;
    drop(buf);
    brelse(bp);
    result
}

pub const ROOTDEV: u32 = 1;
pub const ROOTINO: u32 = 1;
pub const FSSIZE: u32 = 2000;

pub const I_DIR: u16 = 1;
pub const I_FILE: u16 = 2;
pub const I_DEV: u16 = 3;