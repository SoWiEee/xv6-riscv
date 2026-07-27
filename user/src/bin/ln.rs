// user/src/bin/ln.rs
#![no_std]
#![no_main]

extern crate alloc;
use xv6_user_lib::syscall;

#[unsafe(no_mangle)]
fn main(argc: usize, argv: *const *const u8) -> isize {
    xv6_user_lib::init_heap();

    let args = unsafe { xv6_user_lib::args(argc, argv) };

    if args.len() != 3 {
        syscall::write(2, b"Usage: ln old new\n");
        return 1;
    }

    if syscall::link(args[1], args[2]) < 0 {
        // Match C xv6's diagnostic; keep it simple since fprintf isn't available.
        syscall::write(2, b"link failed\n");
    }
    0
}
