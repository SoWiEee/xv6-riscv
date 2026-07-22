// kernel/src/fs/file.rs
use crate::fs::inode::Inode;
use crate::fs::pipe::Pipe;
use crate::sync::spinlock::{SpinLock, SpinLockGuard};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    None,
    Pipe,
    Inode,
    Device,
}

pub struct File {
    lock: SpinLock<FileInner>,
}

struct FileInner {
    typ: FileType,
    refcnt: usize,
    readable: bool,
    writable: bool,
    pipe: Option<Pipe>,
    inode: Option<&'static Inode>,
    off: usize,
    major: u16,
}

impl File {
    pub fn new_inode(inode: &'static Inode, readable: bool, writable: bool) -> Self {
        Self {
            lock: SpinLock::new(FileInner {
                typ: FileType::Inode,
                refcnt: 1,
                readable,
                writable,
                pipe: None,
                inode: Some(inode),
                off: 0,
                major: 0,
            }, "file"),
        }
    }
    
    pub fn new_pipe(pipe: Pipe, readable: bool, writable: bool) -> Self {
        Self {
            lock: SpinLock::new(FileInner {
                typ: FileType::Pipe,
                refcnt: 1,
                readable,
                writable,
                pipe: Some(pipe),
                inode: None,
                off: 0,
                major: 0,
            }, "file"),
        }
    }
    
    pub fn new_device(major: u16, readable: bool, writable: bool) -> Self {
        Self {
            lock: SpinLock::new(FileInner {
                typ: FileType::Device,
                refcnt: 1,
                readable,
                writable,
                pipe: None,
                inode: None,
                off: 0,
                major,
            }, "file"),
        }
    }
    
    fn inner(&self) -> SpinLockGuard<FileInner> {
        self.lock.acquire()
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
            let file = File {
                lock: SpinLock::new(FileInner {
                    typ: FileType::None,
                    refcnt: 1,
                    readable: false,
                    writable: false,
                    pipe: None,
                    inode: None,
                    off: 0,
                    major: 0,
                }, "file"),
            };
            ftable.files[i] = Some(file);
            // Return a copy by creating a new one with same data
            return Some(File {
                lock: SpinLock::new(FileInner {
                    typ: FileType::None,
                    refcnt: 1,
                    readable: false,
                    writable: false,
                    pipe: None,
                    inode: None,
                    off: 0,
                    major: 0,
                }, "file"),
            });
        }
    }
    None
}

pub fn fileclose(f: File) {
    let should_close = f.dec_ref();
    if !should_close {
        return;
    }
    
    let mut inner = f.inner();
    match inner.typ {
        FileType::Pipe => {
            if let Some(pipe) = inner.pipe.take() {
                // Pipe close logic handled by pipe itself
            }
        }
        FileType::Inode => {
            if let Some(inode) = inner.inode.take() {
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
    // Create a new File struct that wraps the same underlying data
    // This is a simplified version - in reality we'd use Arc or similar
    let inner = f.inner();
    let typ = inner.typ;
    let readable = inner.readable;
    let writable = inner.writable;
    let pipe = inner.pipe.clone();
    let inode = inner.inode;
    let off = inner.off;
    let major = inner.major;
    drop(inner);
    
    match typ {
        FileType::Pipe => File::new_pipe(pipe.unwrap(), readable, writable),
        FileType::Inode => File::new_inode(inode.unwrap(), readable, writable),
        FileType::Device => File::new_device(major, readable, writable),
        FileType::None => {
            File {
                lock: SpinLock::new(FileInner {
                    typ: FileType::None,
                    refcnt: 1,
                    readable: false,
                    writable: false,
                    pipe: None,
                    inode: None,
                    off: 0,
                    major: 0,
                }, "file"),
            }
        }
    }
}

pub fn fileread(f: &File, dst: &mut [u8]) -> usize {
    let inner = f.inner();
    if !inner.readable {
        return 0;
    }
    
    match inner.typ {
        FileType::Pipe => {
            if let Some(pipe) = &inner.pipe {
                pipe.read(dst)
            } else {
                0
            }
        }
        FileType::Inode => {
            if let Some(inode) = inner.inode {
                let off = inner.off;
                let n = inode.read(dst, off, dst.len());
                drop(inner);
                f.set_off(off + n);
                n
            } else {
                0
            }
        }
        FileType::Device => {
            // Device read - not implemented
            0
        }
        FileType::None => 0,
    }
}

pub fn filewrite(f: &File, src: &[u8]) -> usize {
    let inner = f.inner();
    if !inner.writable {
        return 0;
    }
    
    match inner.typ {
        FileType::Pipe => {
            if let Some(pipe) = &inner.pipe {
                pipe.write(src)
            } else {
                0
            }
        }
        FileType::Inode => {
            if let Some(inode) = inner.inode {
                let off = inner.off;
                let n = inode.write(src, off, src.len());
                drop(inner);
                f.set_off(off + n);
                n
            } else {
                0
            }
        }
        FileType::Device => {
            // Device write - not implemented
            0
        }
        FileType::None => 0,
    }
}

pub fn filestat(f: &File, _addr: usize) -> isize {
    let inner = f.inner();
    match inner.typ {
        FileType::Inode => {
            if inner.inode.is_some() {
                // Fill in stat structure at addr
                // For now, just return success
                0
            } else {
                -1
            }
        }
        _ => -1,
    }
}