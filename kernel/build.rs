// kernel/build.rs
use std::process::Command;
use std::env;

fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=src/arch/asm.S");
    println!("cargo:rerun-if-changed=src/arch/entry.S");
    
    let out_dir = env::var("OUT_DIR").unwrap();
    
    // Compile assembly files
    cc::Build::new()
        .file("src/arch/asm.S")
        .flag("-march=rv64imac_zicsr")
        .flag("-mabi=lp64")
        .compile("xv6asm");
    
    // Compile entry.S to object file
    let entry_obj = format!("{}/entry.o", out_dir);
    let status = Command::new("riscv64-unknown-elf-gcc")
        .args([
            "-march=rv64imac_zicsr",
            "-mabi=lp64",
            "-c",
            "src/arch/entry.S",
            "-o",
            &entry_obj,
        ])
        .status()
        .expect("Failed to compile entry.S");
    assert!(status.success());
    
    // Add the entry object to linker
    println!("cargo:rustc-link-arg={}", entry_obj);
}