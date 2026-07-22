// kernel/src/bin/kernel.rs
#![no_std]
#![no_main]

extern crate alloc;

// Import the kernel library
use xv6_kernel as _;

// The _start function is in the library