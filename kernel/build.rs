// kernel/build.rs
use std::process::Command;
use std::env;

fn main() {
    println!("cargo:rerun-if-changed=memory.x");
    println!("cargo:rerun-if-changed=src/arch/asm.S");
    println!("cargo:rerun-if-changed=src/arch/entry.S");
    
    let out_dir = env::var("OUT_DIR").unwrap();
    
    // Always use the cross-compiler for kernel assembly files since they contain RISC-V specific instructions
    let compiler = "riscv64-unknown-elf-gcc";
    let arch_flag = "rv64imac_zicsr";
    let abi_flag = "lp64";
    
    // Compile assembly files with explicit cross-compiler
    cc::Build::new()
        .compiler(compiler)
        .file("src/arch/asm.S")
        .flag(&format!("-march={}", arch_flag))
        .flag(&format!("-mabi={}", abi_flag))
        .flag("-nostdlib")
        .flag("-static")
        .define("__ASSEMBLY__", None)
        .no_default_flags(true)
        .compile("xv6asm");
    
    // Compile entry.S to object file
    let entry_obj = format!("{}/entry.o", out_dir);
    let status = Command::new(compiler)
        .args([
            &format!("-march={}", arch_flag),
            &format!("-mabi={}", abi_flag),
            "-nostdlib",
            "-static",
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