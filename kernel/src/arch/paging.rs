// kernel/src/arch/paging.rs
use super::asm::{sfence_vma, w_satp, make_satp};
use crate::mm::address::{PhysAddr, VirtAddr, PhysPageNum, VirtPageNum};

pub const PAGE_SIZE: usize = 4096;
pub const VPBITS: usize = 39;
pub const PPBITS: usize = 56;

// Re-export PTE constants
pub use super::registers::{PTE_V, PTE_R, PTE_W, PTE_X, PTE_U, PTE_A, PTE_D};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn new() -> Self { Self(0) }
    pub fn is_valid(&self) -> bool { (self.0 & PTE_V) != 0 }
    pub fn ppn(&self) -> PhysPageNum { PhysPageNum::new(((self.0 >> 10) & ((1 << 44) - 1)) as usize) }
    pub fn set_ppn(&mut self, ppn: PhysPageNum) { self.0 = (self.0 & 0x3FF) | ((ppn.0 as u64) << 10) }
    pub fn flags(&self) -> u64 { self.0 & 0x3FF }
    pub fn set_flags(&mut self, flags: u64) { self.0 = (self.0 & !0x3FF) | flags }
    pub fn is_user(&self) -> bool { (self.0 & PTE_U) != 0 }
    pub fn is_writable(&self) -> bool { (self.0 & PTE_W) != 0 }
}

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self { entries: [PageTableEntry::new(); 512] }
    }

    pub fn as_mut_ptr(&mut self) -> *mut PageTableEntry {
        self.entries.as_mut_ptr()
    }

    pub fn get_mut(&mut self, vpn: VirtPageNum, _level: usize) -> &mut PageTableEntry {
        &mut self.entries[vpn.0 & 0x1FF]
    }
}

#[derive(Clone, Copy)]
pub struct PageTableWalker {
    root: PhysPageNum,
}

impl PageTableWalker {
    pub fn new(root: PhysPageNum) -> Self { Self { root } }

    pub fn walk(&self, vaddr: VirtAddr, alloc: bool) -> Option<&mut PageTableEntry> {
        let mut pt = self.root;
        for level in (1..=2).rev() {
            let vpn = (vaddr.0 >> (12 + level * 9)) & 0x1FF;
            let pte = unsafe { &mut *(Self::pte_ptr(pt, vpn) as *mut PageTableEntry) };
            if pte.is_valid() {
                pt = pte.ppn();
            } else if alloc {
                let new_pt = crate::mm::frame_allocator::alloc_page().ok()?;
                pte.set_ppn(new_pt);
                pte.set_flags(PTE_V);
                // Zero the new page table
                unsafe { core::ptr::write_bytes(Self::pte_ptr(new_pt, 0) as *mut u8, 0, PAGE_SIZE) };
                pt = new_pt;
            } else {
                return None;
            }
        }
        let vpn = (vaddr.0 >> 12) & 0x1FF;
        Some(unsafe { &mut *(Self::pte_ptr(pt, vpn) as *mut PageTableEntry) })
    }

    fn pte_ptr(ppn: PhysPageNum, vpn: usize) -> usize {
        (ppn.0 << 12) + (vpn * 8)
    }

    pub fn map_pages(&mut self, vaddr: VirtAddr, paddr: PhysAddr, pages: usize, flags: u64) -> Result<(), &'static str> {
        for i in 0..pages {
            let pte = self.walk(vaddr + i * PAGE_SIZE, true).ok_or("walk failed")?;
            if pte.is_valid() {
                return Err("remap");
            }
            pte.set_ppn(PhysPageNum::new((paddr.0 + i * PAGE_SIZE) >> 12));
            pte.set_flags(flags | PTE_V);
        }
        Ok(())
    }
}

pub fn kvm_init() -> PhysPageNum {
    let root = crate::mm::frame_allocator::alloc_page().expect("kvm_init: no memory");
    let _walker = PageTableWalker::new(root);
    // Map UART, VIRTIO, PLIC, kernel text/data, trampoline, kernel stacks
    // ... implementation
    crate::mm::paging::set_kernel_pagetable(root);
    root
}

pub fn kvm_init_hart(root: PhysPageNum) {
    w_satp(make_satp(root.0));
    sfence_vma();
}