// kernel/src/fs/pipe.rs
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use crate::proc::{sleep, wakeup};
use alloc::vec::Vec;

pub struct Pipe {
    lock: SpinLock<PipeInner>,
}

struct PipeInner {
    data: Vec<u8>,
    nread: usize,
    nwrite: usize,
    read_open: bool,
    write_open: bool,
}

impl Pipe {
    pub fn new() -> Self {
        Self {
            lock: SpinLock::new(PipeInner {
                data: Vec::with_capacity(512), // PIPE_SIZE
                nread: 0,
                nwrite: 0,
                read_open: true,
                write_open: true,
            }, "pipe"),
        }
    }
    
    fn inner(&self) -> SpinLockGuard<PipeInner> {
        self.lock.acquire()
    }
    
    pub fn read(&self, dst: &mut [u8]) -> usize {
        let mut inner = self.inner();
        
        while inner.nread == inner.nwrite && inner.write_open {
            // Buffer empty and writers still exist - sleep
            let chan = self as *const _ as usize;
            drop(inner); // Release lock before sleep
            sleep(chan, &self.lock);
            inner = self.inner();
        }
        
        if inner.nread == inner.nwrite && !inner.write_open {
            // EOF - no writers
            return 0;
        }
        
        let available = inner.nwrite - inner.nread;
        let n = core::cmp::min(dst.len(), available);
        
        // Copy data
        let src_data = inner.data[inner.nread..inner.nread + n].to_vec();
        dst[..n].copy_from_slice(&src_data);
        inner.nread += n;
        
        // If we've read everything, reset buffer
        if inner.nread == inner.nwrite {
            inner.nread = 0;
            inner.nwrite = 0;
            inner.data.clear();
        }
        
        // Wake up writers
        let chan = self as *const _ as usize;
        wakeup(chan);
        
        n
    }
    
    pub fn write(&self, src: &[u8]) -> usize {
        let mut inner = self.inner();
        
        if !inner.read_open {
            // No readers - broken pipe
            return 0;
        }
        
        let max_size = 512; // PIPE_SIZE
        let available = max_size - (inner.nwrite - inner.nread);
        let n = core::cmp::min(src.len(), available);
        
        if n == 0 {
            // Buffer full - sleep
            let chan = self as *const _ as usize;
            drop(inner);
            sleep(chan, &self.lock);
            inner = self.inner();
            
            // Retry after wakeup
            let available = max_size - (inner.nwrite - inner.nread);
            let n = core::cmp::min(src.len(), available);
            if n == 0 {
                return 0;
            }
        }
        
        // Ensure capacity
        if inner.data.len() < inner.nwrite + n {
            inner.data.resize(inner.nwrite + n, 0);
        }
        
        let dst_start = inner.nwrite;
        let dst_end = inner.nwrite + n;
        // Copy data - need to avoid overlapping borrows
        let src_slice = &src[..n];
        for (i, &byte) in src_slice.iter().enumerate() {
            inner.data[dst_start + i] = byte;
        }
        inner.nwrite += n;
        
        // Wake up readers
        let chan = self as *const _ as usize;
        wakeup(chan);
        
        n
    }
    
    pub fn read_close(&self) {
        let mut inner = self.inner();
        inner.read_open = false;
        let chan = self as *const _ as usize;
        wakeup(chan);
    }
    
    pub fn write_close(&self) {
        let mut inner = self.inner();
        inner.write_open = false;
        let chan = self as *const _ as usize;
        wakeup(chan);
    }
}

impl Clone for Pipe {
    fn clone(&self) -> Self {
        let inner = self.inner();
        Self {
            lock: SpinLock::new(PipeInner {
                data: inner.data.clone(),
                nread: inner.nread,
                nwrite: inner.nwrite,
                read_open: inner.read_open,
                write_open: inner.write_open,
            }, "pipe"),
        }
    }
}