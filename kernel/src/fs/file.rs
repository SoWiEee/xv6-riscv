// kernel/src/fs/file.rs
use crate::fs::inode::Inode;
use crate::fs::pipe::Pipe;
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use alloc::sync::Arc;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    None,
    Pipe,
    Inode,
    Device,
}

pub struct File {
    inner: Arc<SpinLock<FileInner>>,
}

pub struct FileInner {
    pub typ: FileType,
    pub refcnt: usize,
    pub readable: bool,
    pub writable: bool,
    pub pipe: Option<Pipe>,
    pub inode: Option<&'static Inode>,
    pub off: usize,
    pub major: u16,
}

impl File {
    fn new_with_inner(inner: Arc<SpinLock<FileInner>>) -> Self {
        Self { inner }
    }
    
    pub fn new_inode(inode: &'static Inode, readable: bool, writable: bool) -> Self {
        Self::new_with_inner(Arc::new(SpinLock::new(FileInner {
            typ: FileType::Inode,
            refcnt: 1,
            readable,
            writable,
            pipe: None,
            inode: Some(inode),
            off: 0,
            major: 0,
        }, "file")))
    }
    
    pub fn new_pipe(pipe: Pipe, readable: bool, writable: bool) -> Self {
        Self::new_with_inner(Arc::new(SpinLock::new(FileInner {
            typ: FileType::Pipe,
            refcnt: 1,
            readable,
            writable,
            pipe: Some(pipe),
            inode: None,
            off: 0,
            major: 0,
        }, "file")))
    }
    
    pub fn new_device(major: u16, readable: bool, writable: bool) -> Self {
        Self::new_with_inner(Arc::new(SpinLock::new(FileInner {
            typ: FileType::Device,
            refcnt: 1,
            readable,
            writable,
            pipe: None,
            inode: None,
            off: 0,
            major,
        }, "file")))
    }
    
    pub fn inner(&self) -> SpinLockGuard<FileInner> {
        self.inner.acquire()
    }
    
    pub fn typ(&self) -> FileType {
        self.inner().typ
    }
    
    pub fn readable(&self) -> bool {
        self.inner().readable
    }
    
    pub fn writable(&self) -> bool {
        self.inner().writable
    }
    
    pub fn inc_ref(&self) {
        let mut inner = self.inner();
        inner.refcnt += 1;
    }
    
    pub fn dec_ref(&self) -> bool {
        let mut inner = self.inner();
        inner.refcnt -= 1;
        inner.refcnt == 0
    }
    
    pub fn pipe(&self) -> Option<Pipe> {
        self.inner().pipe.clone()
    }
    
    pub fn inode(&self) -> Option<&'static Inode> {
        self.inner().inode
    }
    
    pub fn off(&self) -> usize {
        self.inner().off
    }
    
    pub fn set_off(&self, off: usize) {
        let mut inner = self.inner();
        inner.off = off;
    }
    
    pub fn major(&self) -> u16 {
        self.inner().major
    }
}

const NFILE: usize = 100;

static FILE_TABLE: SpinLock<FileTable> = SpinLock::new(FileTable::new(), "ftable");

struct FileTable {
    files: [Option<File>; NFILE],
}

impl FileTable {
    const fn new() -> Self {
        Self { files: [const { None }; NFILE] }
    }
}

pub fn fileinit() {
    let mut ftable = FILE_TABLE.acquire();
    for i in 0..NFILE {
        ftable.files[i] = None;
    }
}

pub fn filealloc() -> Option<File> {
    let mut ftable = FILE_TABLE.acquire();
    for i in 0..NFILE {
        if ftable.files[i].is_none() {
            let inner = Arc::new(SpinLock::new(FileInner {
                typ: FileType::None,
                refcnt: 1,
                readable: false,
                writable: false,
                pipe: None,
                inode: None,
                off: 0,
                major: 0,
            }, "file"));
            
            let file = File::new_with_inner(Arc::clone(&inner));
            ftable.files[i] = Some(file);
            return Some(File::new_with_inner(inner));
        }
    }
    None
}

pub fn fileclose(f: &File) {
    let should_close = f.dec_ref();
    if !should_close {
        return;
    }
    
    let mut inner = f.inner();
    let typ = inner.typ;
    let readable = inner.readable;
    let writable = inner.writable;
    let pipe = inner.pipe.take();
    let inode = inner.inode.take();
    drop(inner);
    
    match typ {
        FileType::Pipe => {
            if let Some(pipe) = pipe {
                if readable && !writable {
                    // Read end closed
                    pipe.read_close();
                } else if writable && !readable {
                    // Write end closed
                    pipe.write_close();
                }
            }
        }
        FileType::Inode => {
            if let Some(inode) = inode {
                crate::fs::iput(inode);
            }
        }
        FileType::Device => {
            // Device close - nothing special
        }
        FileType::None => {}
    }
}

pub fn filedup(f: &File) -> File {
    f.inc_ref();
    File::new_with_inner(Arc::clone(&f.inner))
}

pub fn fileread(f: &File, dst: &mut [u8]) -> usize {
    // Snapshot the fields we need, then RELEASE the FileInner lock before any
    // blocking call. `pipe.read` (and inode/device I/O) can sleep; holding this
    // lock across a sleep would let the sleeping process block anyone else who
    // touches the same File — e.g. a peer that shares this fd via fork and calls
    // fileclose (`dec_ref` locks FileInner). That is a spin-forever deadlock
    // with interrupts off. xv6 likewise never holds the file struct lock across
    // pipe/inode I/O.
    let (readable, typ, pipe, inode, off) = {
        let inner = f.inner();
        (inner.readable, inner.typ, inner.pipe.clone(), inner.inode, inner.off)
    };
    if !readable {
        return 0;
    }

    match typ {
        FileType::Pipe => match pipe {
            Some(pipe) => pipe.read(dst),
            None => 0,
        },
        FileType::Inode => match inode {
            Some(inode) => {
                let n = inode.read(dst, off, dst.len());
                f.set_off(off + n);
                n
            }
            None => 0,
        },
        FileType::Device => {
            // Route to the backing device. Only the console (major 1) exists.
            crate::drivers::console::console_read(dst)
        }
        FileType::None => 0,
    }
}

pub fn filewrite(f: &File, src: &[u8]) -> usize {
    // Release the FileInner lock before any blocking call (see fileread for why
    // holding it across pipe/inode I/O deadlocks a shared File).
    let (writable, typ, pipe, inode, off) = {
        let inner = f.inner();
        (inner.writable, inner.typ, inner.pipe.clone(), inner.inode, inner.off)
    };
    if !writable {
        return 0;
    }

    match typ {
        FileType::Pipe => match pipe {
            Some(pipe) => pipe.write(src),
            None => 0,
        },
        FileType::Inode => match inode {
            Some(inode) => {
                let n = inode.write(src, off, src.len());
                f.set_off(off + n);
                n
            }
            None => 0,
        },
        FileType::Device => {
            // Route to the backing device. Only the console (major 1) exists.
            crate::drivers::console::console_write(src)
        }
        FileType::None => 0,
    }
}

/// Fill the C xv6 `struct stat` at `addr` (a kernel-reachable physical address;
/// the caller translates the user pointer). Layout MUST match `Stat` in
/// user-lib: dev:i32@0, ino:u32@4, type:i16@8, nlink:i16@10, size:u64@16.
pub fn filestat(f: &File, addr: usize) -> isize {
    let inner = f.inner();
    let inode = match inner.inode {
        Some(inode) => inode,
        None => return -1,
    };

    inode.lock();
    // InodeType is numbered to match xv6 (Dir=1, File=2, Device=3), so it maps
    // straight onto the on-disk stat `type` field.
    let kind = inode.typ() as i16;
    let dev = inode.dev() as i32;
    let ino = inode.inum();
    let nlink = inode.nlink() as i16;
    let size = inode.size() as u64;
    inode.unlock();

    let stat_ptr = addr as *mut u8;
    unsafe {
        *(stat_ptr.add(0) as *mut i32) = dev;
        *(stat_ptr.add(4) as *mut u32) = ino;
        *(stat_ptr.add(8) as *mut i16) = kind;
        *(stat_ptr.add(10) as *mut i16) = nlink;
        *(stat_ptr.add(16) as *mut u64) = size;
    }
    0
}