// kernel/src/fs/file.rs
use crate::fs::inode::{Inode, InodeType};
use crate::fs::pipe::Pipe;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    Pipe,
    Inode,
    Device,
}

#[derive(Clone)]
pub struct File {
    pub typ: FileType,
    pub readable: bool,
    pub writable: bool,
    pub pipe: Option<Pipe>,
    pub inode: Option<Inode>,
    pub offset: usize,
    pub major: u32,
}

impl File {
    pub fn new_inode(inode: Inode, readable: bool, writable: bool) -> Self {
        Self {
            typ: FileType::Inode,
            readable,
            writable,
            pipe: None,
            inode: Some(inode),
            offset: 0,
            major: 0,
        }
    }
    
    pub fn new_pipe(pipe: Pipe, readable: bool, writable: bool) -> Self {
        Self {
            typ: FileType::Pipe,
            readable,
            writable,
            pipe: Some(pipe),
            inode: None,
            offset: 0,
            major: 0,
        }
    }
    
    pub fn new_device(major: u32, readable: bool, writable: bool) -> Self {
        Self {
            typ: FileType::Device,
            readable,
            writable,
            pipe: None,
            inode: None,
            offset: 0,
            major,
        }
    }
}

pub fn filealloc() -> Option<File> {
    // Allocate a file structure
    None
}

pub fn fileclose(_f: File) {
    // Close file
}

pub fn filedup(f: &File) -> File {
    f.clone()
}

pub fn fileread(_f: File, _dst: &mut [u8]) -> usize {
    // Read from file
    0
}

pub fn filewrite(_f: File, _src: &[u8]) -> usize {
    // Write to file
    0
}