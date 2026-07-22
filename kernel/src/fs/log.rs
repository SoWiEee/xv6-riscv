// kernel/src/fs/log.rs
use crate::fs::buf::{bread, bwrite, brelse, BSIZE};
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::sync::condvar::Condvar;
use alloc::vec::Vec;

const LOGSIZE: usize = 30;
const LOG_MAGIC: u32 = 0x584C4F47; // "XLOG"

struct Log {
    lock: SpinLock<LogInner>,
    committed: Condvar,
}

struct LogInner {
    dev: u32,
    start: u32,
    size: u32,
    outstanding: usize,
    committing: bool,
    lh: LogHeader,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct LogHeader {
    n: u32,
    block: [u32; LOGSIZE],
}

static LOG: Log = Log::new();

impl Log {
    const fn new() -> Self {
        Self {
            lock: SpinLock::new(LogInner {
                dev: 0, start: 0, size: 0,
                outstanding: 0, committing: false,
                lh: LogHeader { n: 0, block: [0; LOGSIZE] },
            }, "log"),
            committed: Condvar::new("log"),
        }
    }
    
    fn lock(&self) -> SpinLockGuard<LogInner> {
        self.lock.acquire()
    }
}

pub fn initlog(dev: u32, sb: &SuperBlock) {
    let mut log = LOG.lock();
    log.dev = dev;
    log.start = sb.logstart;
    log.size = sb.nlog;
}

pub fn begin_op() {
    let mut log = LOG.lock();
    log.outstanding += 1;
}

pub fn end_op() {
    let mut log = LOG.lock();
    log.outstanding -= 1;
    if log.outstanding == 0 && log.committing {
        LOG.committed.wakeup();
    }
}

fn write_log() {
    let mut log = LOG.lock();
    for i in 0..log.lh.n as usize {
        let bp = bread(log.dev, log.start + 1 + i as u32);
        let src_bp = bread(log.dev, log.lh.block[i]);
        {
            let src_buf = src_bp.lock();
            let mut dst_buf = bp.lock();
            dst_buf.data_mut().copy_from_slice(src_buf.data());
        }
        bwrite(&bp);
        brelse(bp);
        brelse(src_bp);
    }
}

fn install_trans() {
    let mut log = LOG.lock();
    for i in 0..log.lh.n as usize {
        let src_bp = bread(log.dev, log.start + 1 + i as u32);
        let dst_bp = bread(log.dev, log.lh.block[i]);
        {
            let src_buf = src_bp.lock();
            let mut dst_buf = dst_bp.lock();
            dst_buf.data_mut().copy_from_slice(src_buf.data());
        }
        bwrite(&dst_bp);
        brelse(dst_bp);
        brelse(src_bp);
    }
}

pub fn recover_from_log() {
    let mut log = LOG.lock();
    // Read log header
    let bp = bread(log.dev, log.start);
    {
        let buf = bp.lock();
        let data = buf.data();
        log.lh = unsafe { *(data.as_ptr() as *const LogHeader) };
    }
    brelse(bp);
    
    if log.lh.n > 0 {
        // Replay log
        install_trans();
        log.lh.n = 0;
        // Write back empty log header
        let bp = bread(log.dev, log.start);
        {
            let mut buf = bp.lock();
            let data = buf.data_mut();
            unsafe { *(data.as_mut_ptr() as *mut LogHeader) = log.lh };
        }
        bwrite(&bp);
        brelse(bp);
    }
}

// SuperBlock definition (also used by inode.rs)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SuperBlock {
    pub magic: u32,
    pub size: u32,
    pub nblocks: u32,
    pub ninodes: u32,
    pub nlog: u32,
    pub logstart: u32,
    pub inodestart: u32,
    pub bmapstart: u32,
}