// kernel/src/mm/page_table.rs
use super::address::{PhysAddr, PhysPageNum, VirtAddr};
use super::frame_allocator::{alloc_page, free_page};
use crate::arch::paging::{PageTableEntry, PageTableWalker, PAGE_SIZE, PTE_V, PTE_R, PTE_W, PTE_X, PTE_U};
use crate::arch::asm::sfence_vma;

#[derive(Clone)]
pub struct PageTable {
    root_ppn: PhysPageNum,
    walker: PageTableWalker,
}

impl PageTable {
    pub fn new() -> Result<Self, &'static str> {
        let root = alloc_page()?;
        // Zero the page
        unsafe { core::ptr::write_bytes(root.to_paddr().0 as *mut u8, 0, PAGE_SIZE) };
        let walker = PageTableWalker::new(root);
        Ok(Self { root_ppn: root, walker })
    }
    
    pub fn from_root(root_ppn: PhysPageNum) -> Self {
        Self { root_ppn, walker: PageTableWalker::new(root_ppn) }
    }
    
    pub fn root_ppn(&self) -> PhysPageNum { self.root_ppn }
    
    pub fn map(&mut self, vaddr: VirtAddr, paddr: PhysAddr, flags: u64) -> Result<(), &'static str> {
        let pte = self.walker.walk(vaddr, true).ok_or("walk failed")?;
        if pte.is_valid() { return Err("remap"); }
        pte.set_ppn(PhysPageNum::new(paddr.0 >> 12));
        pte.set_flags(flags | PTE_V);
        Ok(())
    }
    
    pub fn unmap(&mut self, vaddr: VirtAddr) {
        if let Some(pte) = self.walker.walk(vaddr, false) {
            if pte.is_valid() {
                let ppn = pte.ppn();
                pte.0 = 0;
                free_page(ppn);
            }
        }
    }
    
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let pte = self.walker.walk(vaddr, false)?;
        if !pte.is_valid() { return None; }
        let pa = pte.ppn().to_paddr().0 + vaddr.0 % PAGE_SIZE;
        Some(PhysAddr(pa))
    }
    
    pub fn copy_from(&mut self, src: &PageTable, size: usize) -> Result<(), &'static str> {
        // Deep copy of user page table
        for vpn in 0..(size / PAGE_SIZE) {
            let vaddr = VirtAddr(vpn * PAGE_SIZE);
            if let Some(src_pte) = src.walker.walk(vaddr, false) {
                if src_pte.is_valid() {
                    let pa = src_pte.ppn().to_paddr();
                    let flags = src_pte.flags();
                    let new_page = alloc_page()?;
                    unsafe { core::ptr::copy_nonoverlapping(pa.0 as *const u8, new_page.to_paddr().0 as *mut u8, PAGE_SIZE) };
                    self.map(vaddr, new_page.to_paddr(), flags)?;
                }
            }
        }
        Ok(())
    }
    
    pub fn activate(&self) {
        crate::arch::asm::w_satp(crate::arch::asm::MAKE_SATP(self.root_ppn.0));
        sfence_vma();
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        // Recursively free all page table pages
        self.free_walk(self.root_ppn);
    }
}

impl PageTable {
    fn free_walk(&mut self, ppn: PhysPageNum) {
        let pt = unsafe { &mut *(ppn.to_paddr().0 as *mut [PageTableEntry; 512]) };
        for pte in pt {
            if pte.is_valid() && (pte.flags() & (PTE_R | PTE_W | PTE_X)) == 0 {
                // Points to next level page table
                self.free_walk(pte.ppn());
            } else if pte.is_valid() {
                // Leaf mapping - free physical page
                free_page(pte.ppn());
            }
            pte.0 = 0;
        }
        free_page(ppn);
    }
}

use spin::Once;

pub static KERNEL_PAGETABLE: Once<PageTable> = Once::new();

pub fn kernel_pagetable() -> PhysPageNum {
    KERNEL_PAGETABLE.get().unwrap().root_ppn()
}

pub fn kvminit() {
    let mut pt = PageTable::new().expect("kvminit: failed to create kernel page table");
    // Map devices, kernel text/data, trampoline, kernel stacks
    map_kernel(&mut pt);
    KERNEL_PAGETABLE.call_once(|| pt);
}

fn map_kernel(pt: &mut PageTable) {
    // UART
    pt.map(VirtAddr(0x10000000), PhysAddr(0x10000000), PTE_R | PTE_W).unwrap();
    // VIRTIO
    pt.map(VirtAddr(0x10001000), PhysAddr(0x10001000), PTE_R | PTE_W).unwrap();
    // PLIC
    pt.map(VirtAddr(0x0C000000), PhysAddr(0x0C000000), PTE_R | PTE_W).unwrap();
    // Kernel text (read-only, executable)
    unsafe extern "C" {
        fn etext();
        fn end();
    }
    let etext = etext as *const () as usize;
    let end = end as *const () as usize;
    let text_start = crate::arch::asm::KERNBASE;
    
    // Map kernel text (RX)
    let text_pages = (etext - text_start + PAGE_SIZE - 1) / PAGE_SIZE;
    for i in 0..text_pages {
        let vaddr = VirtAddr(text_start + i * PAGE_SIZE);
        let paddr = PhysAddr(text_start + i * PAGE_SIZE);
        pt.map(vaddr, paddr, PTE_R | PTE_X).unwrap();
    }
    
    // Map kernel data (RW)
    let data_start = (etext + PAGE_SIZE - 1) & !(PAGE_SIZE - 1); // Page align up
    let data_pages = (end - data_start + PAGE_SIZE - 1) / PAGE_SIZE;
    for i in 0..data_pages {
        let vaddr = VirtAddr(data_start + i * PAGE_SIZE);
        let paddr = PhysAddr(data_start + i * PAGE_SIZE);
        pt.map(vaddr, paddr, PTE_R | PTE_W).unwrap();
    }
    
    // Trampoline
    pt.map(VirtAddr(crate::arch::asm::TRAMPOLINE), PhysAddr(crate::arch::asm::TRAMPOLINE), PTE_R | PTE_X).unwrap();
    
    // Kernel stacks for each CPU - map 1MB for stacks
    let stack_top = crate::arch::asm::PHYSTOP;
    let stack_pages = 1 * 1024 * 1024 / PAGE_SIZE;
    for i in 0..stack_pages {
        let vaddr = VirtAddr(stack_top - (i + 1) * PAGE_SIZE);
        let paddr = PhysAddr(stack_top - (i + 1) * PAGE_SIZE);
        pt.map(vaddr, paddr, PTE_R | PTE_W).unwrap();
    }
}

pub fn kvminithart() {
    KERNEL_PAGETABLE.get().unwrap().activate();
}

// For user page table creation
pub fn uvmcreate() -> Result<PageTable, &'static str> {
    let mut pt = PageTable::new()?;
    // Map trampoline
    pt.map(VirtAddr(crate::arch::asm::TRAMPOLINE), PhysAddr(crate::arch::asm::TRAMPOLINE), PTE_R | PTE_X).unwrap();
    Ok(pt)
}

pub fn uvmalloc(pt: &mut PageTable, old_sz: usize, new_sz: usize) -> Result<(), &'static str> {
    if new_sz <= old_sz {
        return Ok(());
    }
    let old_end = (old_sz + PAGE_SIZE - 1) / PAGE_SIZE * PAGE_SIZE;
    for vaddr in (old_end..new_sz).step_by(PAGE_SIZE) {
        let page = alloc_page()?;
        pt.map(VirtAddr(vaddr), page.to_paddr(), PTE_R | PTE_W | PTE_U | PTE_X)?;
    }
    Ok(())
}

pub fn uvmfree(pt: &mut PageTable, sz: usize) {
    for vaddr in (0..sz).step_by(PAGE_SIZE) {
        pt.unmap(VirtAddr(vaddr));
    }
}

pub fn uvmcopy(src: &PageTable, dst: &mut PageTable, sz: usize) -> Result<(), &'static str> {
    for vaddr in (0..sz).step_by(PAGE_SIZE) {
        let va = VirtAddr(vaddr);
        if let Some(pte) = src.walker.walk(va, false) {
            if pte.is_valid() {
                let pa = pte.ppn().to_paddr();
                let flags = pte.flags();
                let new_page = alloc_page()?;
                unsafe { core::ptr::copy_nonoverlapping(pa.0 as *const u8, new_page.to_paddr().0 as *mut u8, PAGE_SIZE) };
                dst.map(va, new_page.to_paddr(), flags)?;
            }
        }
    }
    Ok(())
}