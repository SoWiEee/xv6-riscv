// kernel/src/fs/pipe.rs
use crate::proc::{sleep, wakeup};
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use alloc::sync::Arc;
use alloc::vec::Vec;

/// A pipe endpoint handle. Both the read and write `File`s of a pipe hold a
/// `Pipe`, and cloning one shares the SAME underlying buffer and lock via `Arc`
/// — cloning must NOT create an independent copy, or bytes written on one end
/// would never be visible on the other.
#[derive(Clone)]
pub struct Pipe {
    inner: Arc<SpinLock<PipeInner>>,
}

struct PipeInner {
    data: Vec<u8>,
    nread: usize,
    nwrite: usize,
    read_open: bool,
    write_open: bool,
}

/// Pipe buffer capacity (matches C xv6 PIPESIZE).
const PIPE_SIZE: usize = 512;

impl Pipe {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SpinLock::new(
                PipeInner {
                    data: Vec::with_capacity(PIPE_SIZE),
                    nread: 0,
                    nwrite: 0,
                    read_open: true,
                    write_open: true,
                },
                "pipe",
            )),
        }
    }

    fn inner(&self) -> SpinLockGuard<PipeInner> {
        self.inner.acquire()
    }

    /// Sleep/wakeup channel: the address of the SHARED inner, so both endpoints
    /// (and every clone) agree on the same channel.
    fn chan(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }

    pub fn read(&self, dst: &mut [u8]) -> usize {
        let mut inner = self.inner();

        // Block while the buffer is empty and writers still exist.
        while inner.nread == inner.nwrite && inner.write_open {
            let chan = self.chan();
            // Hand the lock to sleep, which releases it atomically as we sleep.
            core::mem::forget(inner);
            sleep(chan, &*self.inner);
            inner = self.inner();
        }

        if inner.nread == inner.nwrite && !inner.write_open {
            return 0; // EOF - no writers
        }

        let available = inner.nwrite - inner.nread;
        let n = core::cmp::min(dst.len(), available);

        let src_data = inner.data[inner.nread..inner.nread + n].to_vec();
        dst[..n].copy_from_slice(&src_data);
        inner.nread += n;

        // Drained: reset the buffer to the front.
        if inner.nread == inner.nwrite {
            inner.nread = 0;
            inner.nwrite = 0;
            inner.data.clear();
        }

        wakeup(self.chan()); // wake any blocked writers
        n
    }

    pub fn write(&self, src: &[u8]) -> usize {
        let mut inner = self.inner();
        let mut total = 0;

        while total < src.len() {
            if !inner.read_open {
                // No readers left: the write fails (broken pipe).
                break;
            }

            let used = inner.nwrite - inner.nread;
            if used >= PIPE_SIZE {
                // Buffer full: wake readers and sleep until space frees up.
                wakeup(self.chan());
                let chan = self.chan();
                core::mem::forget(inner);
                sleep(chan, &*self.inner);
                inner = self.inner();
                continue;
            }

            let space = PIPE_SIZE - used;
            let n = core::cmp::min(src.len() - total, space);

            let needed = inner.nwrite + n;
            if inner.data.len() < needed {
                inner.data.resize(needed, 0);
            }
            let dst_start = inner.nwrite;
            inner.data[dst_start..dst_start + n].copy_from_slice(&src[total..total + n]);
            inner.nwrite += n;
            total += n;
        }

        wakeup(self.chan()); // wake any blocked readers
        total
    }

    pub fn read_close(&self) {
        let mut inner = self.inner();
        inner.read_open = false;
        drop(inner);
        wakeup(self.chan());
    }

    pub fn write_close(&self) {
        let mut inner = self.inner();
        inner.write_open = false;
        drop(inner);
        wakeup(self.chan());
    }
}
