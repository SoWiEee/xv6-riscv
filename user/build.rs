// user/build.rs
//
// Link every user program with the xv6 user linker script (loaded at 0x10000,
// ENTRY(main)) instead of the kernel's memory.x. This is required because the
// user crate shares the riscv64imac-unknown-none-elf target with the kernel,
// and .cargo/config.toml deliberately does not set a linker script globally.
use std::env;

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rerun-if-changed=user.ld");
    println!("cargo:rustc-link-arg=-T{}/user.ld", manifest_dir);
}
