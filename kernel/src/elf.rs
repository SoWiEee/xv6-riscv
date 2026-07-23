// kernel/src/elf.rs
//! ELF parsing and loading for RISC-V 64-bit.

use crate::fs::{File, fileread};
use crate::mm::page_table::{PageTable, uvmalloc};
use crate::mm::address::VirtAddr;
use crate::arch::paging::{PAGE_SIZE, PTE_R, PTE_W, PTE_X, PTE_U, PTE_V};
use alloc::vec::Vec;
use alloc::string::String;
use core::mem;

/// ELF identification constants
const ELFMAG0: u8 = 0x7f;
const ELFMAG1: u8 = b'E';
const ELFMAG2: u8 = b'L';
const ELFMAG3: u8 = b'F';

/// ELF class
const ELFCLASS64: u8 = 2;

/// ELF data encoding
const ELFDATA2LSB: u8 = 1;

/// ELF version
const EV_CURRENT: u8 = 1;

/// ELF OS/ABI
const ELFOSABI_NONE: u8 = 0;

/// ELF type
const ET_EXEC: u16 = 2;

/// Machine type
const EM_RISCV: u16 = 243;

/// Program header types
const PT_LOAD: u32 = 1;

/// Program header flags
const PF_X: u32 = 1;
const PF_W: u32 = 2;
const PF_R: u32 = 4;

/// ELF64 Header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// ELF64 Program Header
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

impl Elf64Ehdr {
    /// Check if the ELF header is valid for RISC-V 64-bit executable
    pub fn check(&self) -> Result<(), &'static str> {
        if self.e_ident[0] != ELFMAG0 || self.e_ident[1] != ELFMAG1 || self.e_ident[2] != ELFMAG2 || self.e_ident[3] != ELFMAG3 {
            return Err("Not an ELF file");
        }
        if self.e_ident[4] != ELFCLASS64 {
            return Err("Not a 64-bit ELF file");
        }
        if self.e_ident[5] != ELFDATA2LSB {
            return Err("Not little-endian");
        }
        if self.e_ident[6] != EV_CURRENT {
            return Err("Invalid ELF version");
        }
        if self.e_ident[7] != ELFOSABI_NONE {
            return Err("Invalid OS/ABI");
        }
        if self.e_type != ET_EXEC {
            return Err("Not an executable");
        }
        if self.e_machine != EM_RISCV {
            return Err("Not RISC-V architecture");
        }
        if self.e_phentsize as usize != mem::size_of::<Elf64Phdr>() {
            return Err("Invalid program header entry size");
        }
        Ok(())
    }
}

impl Elf64Phdr {
    /// Get the protection flags for page table entries
    pub fn pte_flags(&self) -> u64 {
        let mut flags = PTE_V | PTE_U;
        if self.p_flags & PF_R != 0 {
            flags |= PTE_R;
        }
        if self.p_flags & PF_W != 0 {
            flags |= PTE_W;
        }
        if self.p_flags & PF_X != 0 {
            flags |= PTE_X;
        }
        flags
    }
}

/// Load an ELF executable into a page table
/// 
/// # Arguments
/// * `file` - The file containing the ELF executable
/// * `pt` - The page table to load into
/// 
/// # Returns
/// * `Ok(entry_point)` - The entry point address on success
/// * `Err(&str)` - Error message on failure
pub fn load_elf(file: &File, pt: &mut PageTable) -> Result<usize, &'static str> {
    // Read ELF header
    let mut ehdr = Elf64Ehdr {
        e_ident: [0; 16],
        e_type: 0,
        e_machine: 0,
        e_version: 0,
        e_entry: 0,
        e_phoff: 0,
        e_shoff: 0,
        e_flags: 0,
        e_ehsize: 0,
        e_phentsize: 0,
        e_phnum: 0,
        e_shentsize: 0,
        e_shnum: 0,
        e_shstrndx: 0,
    };
    
    // Read the ELF header from the file
    file.set_off(0);
    let bytes_read = fileread(file, unsafe {
        core::slice::from_raw_parts_mut(&mut ehdr as *mut Elf64Ehdr as *mut u8, mem::size_of::<Elf64Ehdr>())
    });
    
    if bytes_read != mem::size_of::<Elf64Ehdr>() {
        return Err("Failed to read ELF header");
    }
    
    // Verify ELF header
    ehdr.check()?;
    
    // Read program headers
    let phdr_size = mem::size_of::<Elf64Phdr>();
    let phdr_count = ehdr.e_phnum as usize;
    let phdr_offset = ehdr.e_phoff as usize;
    
    let mut phdrs = Vec::with_capacity(phdr_count);
    
    for i in 0..phdr_count {
        let mut phdr = Elf64Phdr {
            p_type: 0,
            p_flags: 0,
            p_offset: 0,
            p_vaddr: 0,
            p_paddr: 0,
            p_filesz: 0,
            p_memsz: 0,
            p_align: 0,
        };
        
        let offset = phdr_offset + i * phdr_size;
        file.set_off(offset);
        let bytes_read = fileread(file, unsafe {
            core::slice::from_raw_parts_mut(&mut phdr as *mut Elf64Phdr as *mut u8, phdr_size)
        });
        
        if bytes_read != phdr_size {
            return Err("Failed to read program header");
        }
        
        phdrs.push(phdr);
    }
    
    // Process PT_LOAD segments
    let mut max_addr = 0usize;
    
    for phdr in &phdrs {
        if phdr.p_type != PT_LOAD {
            continue;
        }
        
        let vaddr = phdr.p_vaddr as usize;
        let filesz = phdr.p_filesz as usize;
        let memsz = phdr.p_memsz as usize;
        let offset = phdr.p_offset as usize;
        // Check alignment
        if phdr.p_align > 0 && (vaddr % phdr.p_align as usize) != 0 {
            return Err("Segment alignment mismatch");
        }
        
        // Track the maximum address for sbrk
        let seg_end = vaddr + memsz;
        if seg_end > max_addr {
            max_addr = seg_end;
        }
        
        // Allocate pages for the segment
        let page_start = vaddr & !(PAGE_SIZE - 1);
        let page_end = (vaddr + memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        // Extend the page table to cover this segment
        if uvmalloc(pt, page_start, page_end).is_err() {
            return Err("Failed to allocate memory for segment");
        }
        
        // Read the segment data from file into user memory
        let mut remaining = filesz;
        let mut file_offset = offset;
        let mut va = vaddr;
        
        while remaining > 0 {
            // Translate virtual address to physical
            let pa = match pt.translate(VirtAddr(va)) {
                Some(pa) => pa,
                None => return Err("Failed to translate virtual address"),
            };
            
            let chunk = core::cmp::min(remaining, PAGE_SIZE - (va % PAGE_SIZE));
            let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, chunk) };
            
            file.set_off(file_offset);
            let bytes_read = fileread(file, dst);
            if bytes_read != chunk {
                return Err("Failed to read segment data from file");
            }
            
            remaining -= chunk;
            file_offset += chunk;
            va += chunk;
        }
        
        // Zero the BSS section (memory size > file size)
        if memsz > filesz {
            let bss_start = vaddr + filesz;
            let bss_end = vaddr + memsz;
            let mut va = bss_start;
            
            while va < bss_end {
                let pa = match pt.translate(VirtAddr(va)) {
                    Some(pa) => pa,
                    None => return Err("Failed to translate BSS address"),
                };
                
                let chunk = core::cmp::min(bss_end - va, PAGE_SIZE - (va % PAGE_SIZE));
                let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, chunk) };
                dst.fill(0);
                
                va += chunk;
            }
        }
    }
    
    Ok(ehdr.e_entry as usize)
}

/// Set up user stack with argc, argv, and envp
/// 
/// # Arguments
/// * `pt` - Page table
/// * `args` - Command line arguments
/// * `sp` - Initial stack pointer (top of user stack)
/// 
/// # Returns
/// * `Ok((sp, argv_ptr))` - New stack pointer and argv pointer
/// * `Err(&str)` - Error message
pub fn setup_user_stack(pt: &mut PageTable, args: &[String], sp: usize) -> Result<(usize, usize), &'static str> {
    // Stack grows down. We need to push:
    // - envp (null-terminated array of pointers)
    // - argv (null-terminated array of pointers)
    // - argc
    // - argument strings
    // - environment strings
    
    let mut stack_ptr = sp;
    
    // Push environment strings (none for now)
    let _envp_ptr = 0usize;
    
    // Push argument strings
    let mut argv_ptrs = Vec::new();
    for arg in args.iter().rev() {
        let arg_bytes = arg.as_bytes();
        let arg_len = arg_bytes.len() + 1; // +1 for null terminator
        stack_ptr = (stack_ptr - arg_len) & !(16 - 1); // 16-byte align
        let pa = match pt.translate(VirtAddr(stack_ptr)) {
            Some(pa) => pa,
            None => return Err("Failed to translate stack address"),
        };
        let dst = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut u8, arg_len) };
        dst[..arg_bytes.len()].copy_from_slice(arg_bytes);
        dst[arg_bytes.len()] = 0; // null terminator
        argv_ptrs.push(stack_ptr);
    }
    
    // Push argv array (pointers to argument strings)
    argv_ptrs.reverse();
    let argv_array_size = (argv_ptrs.len() + 1) * mem::size_of::<usize>(); // +1 for null terminator
    stack_ptr = (stack_ptr - argv_array_size) & !(16 - 1);
    let pa = match pt.translate(VirtAddr(stack_ptr)) {
        Some(pa) => pa,
        None => return Err("Failed to translate argv array address"),
    };
    let argv_array = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut usize, argv_ptrs.len() + 1) };
    for (i, &ptr) in argv_ptrs.iter().enumerate() {
        argv_array[i] = ptr;
    }
    argv_array[argv_ptrs.len()] = 0; // null terminator
    let argv_ptr = stack_ptr;
    
    // Push envp array (null for now)
    let envp_array_size = mem::size_of::<usize>(); // just null terminator
    stack_ptr = (stack_ptr - envp_array_size) & !(16 - 1);
    let pa = match pt.translate(VirtAddr(stack_ptr)) {
        Some(pa) => pa,
        None => return Err("Failed to translate envp array address"),
    };
    let envp_array = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut usize, 1) };
    envp_array[0] = 0;
    
    // Push argc
    stack_ptr = (stack_ptr - mem::size_of::<usize>()) & !(16 - 1);
    let pa = match pt.translate(VirtAddr(stack_ptr)) {
        Some(pa) => pa,
        None => return Err("Failed to translate argc address"),
    };
    let argc_ptr = unsafe { core::slice::from_raw_parts_mut(pa.0 as *mut usize, 1) };
    argc_ptr[0] = args.len();
    
    // Stack pointer should be 16-byte aligned
    stack_ptr &= !(16 - 1);
    
    Ok((stack_ptr, argv_ptr))
}