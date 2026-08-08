// kernel/src/fs/log.rs
//! Write-ahead logging for crash recovery.
//!
//! Mirrors C xv6 `kernel/log.c`. A file-system system call brackets all of its
//! disk writes between `begin_op()` and `end_op()`. Instead of writing home
//! locations directly, modified blocks are handed to `log_write`, which records
//! them and pins them in the buffer cache. `end_op` on the last outstanding op
//! commits: the pinned blocks are copied to the on-disk log, the log header is
//! written (the commit point), the blocks are installed to their home
//! locations, and the header is cleared. A crash before the header is written
//! loses the transaction cleanly; a crash after replays it on the next boot via
//! `recover_from_log`.
//!
//! ## Locking discipline
//!
//! `begin_op`/`end_op`/`log_write` mutate the shared `LogInner` under the `LOG`
//! spinlock. `commit()` and the disk helpers (`write_log`, `install_trans`,
//! `write_head`, `read_head`) call `bread`/`bwrite` (sleeplocks) and therefore
//! MUST NOT run while holding the `LOG` spinlock. They are safe to run lock-free
//! because the `committing` flag makes every other op sleep in `begin_op` — only
//! the single committing process touches the log during a commit.

use crate::fs::buf::{bread, brelse, bwrite, bpin, bunpin, BSIZE};

/// Scratch buffer to gather the transaction's log blocks into one contiguous
/// region so `write_log` can flush them in a single virtio request. Only ever
/// touched inside `write_log`, which runs solely from the serialised `commit()`
/// (guarded by the log's `committing` flag), so it needs no lock. Sized for a
/// full log (LOGSIZE blocks); a transaction uses at most MAXOPBLOCKS.
static mut LOG_GATHER: [u8; LOGSIZE * BSIZE] = [0; LOGSIZE * BSIZE];
use crate::sync::spinlock::SpinLock;

/// Max blocks a single file-system op may write. Bounds one transaction so the
/// log can always hold `outstanding * MAXOPBLOCKS` blocks.
pub const MAXOPBLOCKS: usize = 10;

/// On-disk log capacity (data blocks). Matches C xv6 `LOGSIZE`.
const LOGSIZE: usize = 30;

struct Log {
    lock: SpinLock<LogInner>,
}

struct LogInner {
    dev: u32,
    start: u32,
    size: u32,
    /// Number of file-system calls currently executing (in a transaction).
    outstanding: usize,
    /// A commit is in progress: new ops must wait.
    committing: bool,
    lh: LogHeader,
}

/// In-memory / on-disk log header. `n` counts logged blocks; `block[i]` is the
/// home block number of the i-th logged block.
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
            lock: SpinLock::new(
                LogInner {
                    dev: 0,
                    start: 0,
                    size: 0,
                    outstanding: 0,
                    committing: false,
                    lh: LogHeader { n: 0, block: [0; LOGSIZE] },
                },
                "log",
            ),
        }
    }
}

/// Sleep/wakeup channel for the log: the address of the log spinlock, so
/// `begin_op` sleepers and `end_op` wakers agree.
fn log_chan() -> usize {
    &LOG.lock as *const _ as usize
}

pub fn initlog(dev: u32, sb: &SuperBlock) {
    let mut log = LOG.lock.acquire();
    log.dev = dev;
    log.start = sb.logstart;
    log.size = sb.nlog;
}

/// Called at the start of each file-system system call.
///
/// Blocks until a commit is not in progress AND the log has room for this op's
/// worst-case `MAXOPBLOCKS` blocks on top of everything already reserved.
pub fn begin_op() {
    let mut log = LOG.lock.acquire();
    loop {
        if log.committing
            || log.lh.n as usize + (log.outstanding + 1) * MAXOPBLOCKS > LOGSIZE
        {
            // Not safe to start: sleep on the log until an end_op wakes us.
            // Hand the still-locked guard to sleep (forget stops Drop from
            // releasing it), then re-acquire on wakeup.
            let chan = log_chan();
            core::mem::forget(log);
            crate::proc::sleep(chan, &LOG.lock);
            log = LOG.lock.acquire();
        } else {
            log.outstanding += 1;
            break;
        }
    }
}

/// Called at the end of each file-system system call. Commits if this was the
/// last outstanding op.
pub fn end_op() {
    let mut do_commit = false;
    {
        let mut log = LOG.lock.acquire();
        log.outstanding -= 1;
        if log.committing {
            panic!("log: end_op while committing");
        }
        if log.outstanding == 0 {
            do_commit = true;
            log.committing = true;
        } else {
            // begin_op may be waiting for log space now that a reservation freed.
            crate::proc::wakeup(log_chan());
        }
    }

    if do_commit {
        // Commit with NO lock held; the committing flag serialises everyone else.
        commit();
        let mut log = LOG.lock.acquire();
        log.committing = false;
        drop(log);
        crate::proc::wakeup(log_chan());
    }
}

/// Record that buffer `b` must be written by the current transaction.
///
/// Absorbs repeated writes of the same block into one log slot and pins the
/// buffer in the cache so it survives until `install_trans` writes it home.
/// Callers use this in place of `bwrite` for any block that belongs to a
/// transaction.
pub fn log_write(b: &crate::fs::buf::BufRef) {
    let blockno = b.blockno();
    let mut log = LOG.lock.acquire();
    if log.lh.n as usize >= LOGSIZE || log.lh.n >= log.size.saturating_sub(1) {
        panic!("log: transaction too big");
    }
    if log.outstanding == 0 {
        panic!("log_write outside of transaction");
    }

    // Log absorption: if this block is already in the log, reuse its slot.
    let n = log.lh.n as usize;
    let mut i = 0;
    while i < n {
        if log.lh.block[i] == blockno {
            break;
        }
        i += 1;
    }
    log.lh.block[i] = blockno;
    if i == n {
        // Newly added to the log: pin it in the cache.
        bpin(b);
        log.lh.n += 1;
    }
}

/// Perform one commit. Runs lock-free (see module docs).
fn commit() {
    // Snapshot the header and log location under a brief lock.
    let (dev, start, lh) = {
        let log = LOG.lock.acquire();
        (log.dev, log.start, log.lh)
    };

    if lh.n == 0 {
        return;
    }

    write_log(dev, start, &lh); // cache -> on-disk log
    write_head(dev, start, &lh); // header with n>0 = the commit point
    install_trans(dev, start, &lh, false); // on-disk log -> home, unpin

    // Erase the transaction: clear in-memory count and the on-disk header.
    {
        let mut log = LOG.lock.acquire();
        log.lh.n = 0;
    }
    let empty = LogHeader { n: 0, block: [0; LOGSIZE] };
    write_head(dev, start, &empty);
}

/// Copy modified (cached, pinned) blocks from the buffer cache to the log area.
fn write_log(dev: u32, start: u32, lh: &LogHeader) {
    let n = lh.n as usize;
    if n == 0 {
        return;
    }
    // The N log blocks occupy consecutive disk blocks [start+1 .. start+1+N),
    // i.e. consecutive sectors. Gather every block's data into one contiguous
    // buffer and flush it in a SINGLE virtio request instead of one round-trip
    // per block. We also copy each block into its log-block cache buffer so
    // install_trans cache-hits it (rather than re-reading from disk).
    // Build the slice from a raw pointer (not `&mut LOG_GATHER`, which the 2024
    // edition rejects). Safe: commit() is serialised, so this is the only live
    // reference to the static.
    let gather: &mut [u8] = unsafe {
        core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(LOG_GATHER) as *mut u8, LOGSIZE * BSIZE)
    };
    for i in 0..n {
        let to = bread(dev, start + 1 + i as u32); // log block cache buffer
        let from = bread(dev, lh.block[i]); // cached home block
        {
            let src = from.lock();
            let mut dst = to.lock();
            dst.data_mut().copy_from_slice(src.data());
            gather[i * BSIZE..(i + 1) * BSIZE].copy_from_slice(src.data());
        }
        brelse(from);
        brelse(to); // stays cached (data populated) for install_trans
    }
    // One request writes all N log blocks (2N consecutive sectors) at sector
    // (start+1)*2. The gather buffer is contiguous .bss, so it is DMA-safe.
    crate::drivers::virtio::virtio_rw_buf((start as u64 + 1) * 2, &mut gather[..n * BSIZE], true);
}

/// Copy committed blocks from the on-disk log to their home locations. With
/// `recovering == false` also unpins each home buffer pinned by `log_write`.
fn install_trans(dev: u32, start: u32, lh: &LogHeader, recovering: bool) {
    for i in 0..lh.n as usize {
        let lbuf = bread(dev, start + 1 + i as u32); // log block
        let dbuf = bread(dev, lh.block[i]); // home block
        {
            let src = lbuf.lock();
            let mut dst = dbuf.lock();
            dst.data_mut().copy_from_slice(src.data());
        }
        bwrite(&dbuf);
        if !recovering {
            bunpin(&dbuf);
        }
        brelse(lbuf);
        brelse(dbuf);
    }
}

/// Write the in-memory log header to disk. Writing a header with `n > 0` is the
/// atomic commit point of the transaction.
fn write_head(dev: u32, start: u32, lh: &LogHeader) {
    let bp = bread(dev, start);
    {
        let mut buf = bp.lock();
        let data = buf.data_mut();
        // SAFETY: BSIZE (1024) >= size_of::<LogHeader>() and the buffer is
        // suitably aligned for u32 access.
        unsafe {
            *(data.as_mut_ptr() as *mut LogHeader) = *lh;
        }
    }
    bwrite(&bp);
    brelse(bp);
}

/// Read the on-disk log header.
fn read_head(dev: u32, start: u32) -> LogHeader {
    let bp = bread(dev, start);
    let lh = {
        let buf = bp.lock();
        // SAFETY: as in write_head; we read a LogHeader-sized prefix.
        unsafe { *(buf.data().as_ptr() as *const LogHeader) }
    };
    brelse(bp);
    lh
}

/// Replay a committed-but-not-installed transaction at boot, then clear the log.
///
/// Runs before the scheduler starts (single-threaded), so it never sleeps and
/// holds no spinlock across the block I/O.
pub fn recover_from_log() {
    let (dev, start) = {
        let log = LOG.lock.acquire();
        (log.dev, log.start)
    };

    let lh = read_head(dev, start);
    if lh.n > 0 {
        install_trans(dev, start, &lh, true); // recovering: do not unpin
        let empty = LogHeader { n: 0, block: [0; LOGSIZE] };
        write_head(dev, start, &empty);
    }

    let mut log = LOG.lock.acquire();
    log.lh.n = 0;
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
