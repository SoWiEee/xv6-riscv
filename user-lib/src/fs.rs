// user-lib/src/fs.rs
use crate::syscall::{self, *};
use alloc::string::String;
use alloc::vec::Vec;

pub const O_RDONLY: i32 = 0x000;
pub const O_WRONLY: i32 = 0x001;
pub const O_RDWR: i32 = 0x002;
pub const O_CREATE: i32 = 0x200;
pub const O_TRUNC: i32 = 0x400;

pub type Stat = syscall::Stat;

pub fn stat(path: &str) -> Result<Stat, isize> {
    let mut st = Stat::default();
    let ret = open(path, O_RDONLY);
    if ret < 0 {
        return Err(ret);
    }
    let fd = ret as i32;
    let ret = fstat(fd, &mut st);
    close(fd);
    if ret < 0 {
        return Err(ret);
    }
    Ok(st)
}

pub fn ls(path: &str) -> Result<Vec<String>, isize> {
    let result = Vec::new();
    let fd = open(path, O_RDONLY);
    if fd < 0 {
        return Err(fd);
    }
    
    let mut st = Stat::default();
    if fstat(fd as i32, &mut st) < 0 {
        close(fd as i32);
        return Err(-1);
    }
    
    if st.type_() != T_DIR {
        close(fd as i32);
        return Err(-1);
    }
    
    // Read directory entries
    // For simplicity, we'll just return an empty vec for now
    // Real implementation would read directory entries
    
    close(fd as i32);
    Ok(result)
}

pub const T_DIR: u16 = 1;
pub const T_FILE: u16 = 2;
pub const T_DEVICE: u16 = 3;