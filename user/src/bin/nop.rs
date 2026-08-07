// user/src/bin/nop.rs
//
// Minimal exec target: does nothing and exits. Used by forkbench's exec sibling
// (execbench) so the measured cost is exec itself, not any work the child does.
#![no_std]
#![no_main]

use xv6_user_lib::syscall;

#[unsafe(no_mangle)]
fn main(_argc: usize, _argv: *const *const u8) -> isize {
    syscall::exit(0);
}
