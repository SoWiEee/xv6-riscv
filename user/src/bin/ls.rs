// user/src/bin/ls.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::{println, syscall, fs};

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    // Initialize heap
    xv6_user_lib::init_heap();

    // Default to the current directory; otherwise list the first argument.
    let args = unsafe { xv6_user_lib::args(argc, argv) };
    let path = if args.len() >= 2 { args[1] } else { "." };

    let fd = syscall::open(path, fs::O_RDONLY);
    if fd < 0 {
        println!("ls: cannot open {}", path);
        return -1;
    }
    
    let mut st = fs::Stat::default();
    if syscall::fstat(fd as i32, &mut st) < 0 {
        println!("ls: cannot stat {}", path);
        syscall::close(fd as i32);
        return -1;
    }
    
    if st.type_() != fs::T_DIR {
        // Print file info
        println!("{}  {}  {}", path, st.size, st.ino);
        syscall::close(fd as i32);
        return 0;
    }
    
    // Read directory entries
    let mut buf = [0u8; 16];
    loop {
        let n = syscall::read(fd as i32, &mut buf);
        if n <= 0 {
            break;
        }
        
        // Parse dirent entries (inum + name)
        let mut i = 0;
        while i + 2 <= n as usize {
            let inum = u16::from_le_bytes([buf[i], buf[i+1]]) as usize;
            i += 2;
            
            if inum == 0 {
                i += 14; // skip name
                continue;
            }
            
            let name_end = i + 14;
            if name_end > n as usize {
                break;
            }
            
            let name_bytes = &buf[i..name_end];
            let name = core::str::from_utf8(name_bytes)
                .unwrap_or("")
                .trim_end_matches('\0');
            
            if !name.is_empty() && name != "." && name != ".." {
                println!("{}", name);
            }
            
            i = name_end;
        }
    }
    
    syscall::close(fd as i32);
    0
}