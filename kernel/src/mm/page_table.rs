// kernel/src/mm/page_table.rs
//! Sv39 page table management.
//!
//! Implements 3-level page tables (Sv39) with 4KB pages. Each level has 512 entries.
//! Page table entries are 64-bit with standard RISC-V flags.

use super::address::{PhysAddr, PhysPageNum, VirtAddr};
use super::frame_allocator::{alloc_page, free_page};
use crate::arch::paging::{PageTableEntry, PageTableWalker, PAGE_SIZE, PTE_V, PTE_R, PTE_W, PTE_X, PTE_U, PTE_A, PTE_D};
use crate::arch::asm::sfence_vma;
use alloc::boxed::Box;

unsafe extern "C" {
    #[link_name = "uservec"]
    fn uservec();
}

/// A Sv39 page table.
///
/// An **owning** `PageTable` (from [`PageTable::new`]) owns the root page and all
/// descendant pages; dropping it recursively frees every page-table page and
/// mapped physical page.
///
/// A **borrowing** view ([`PageTable::from_root`] or [`Clone`]) refers to an
/// existing tree without owning it: `owned == false`, so dropping it frees
/// nothing. This is essential — constructing a throwaway view over a live
/// process page table (e.g. to `translate` an address) must NOT tear down that
/// process's address space when the view goes out of scope.
///
/// # Example
/// ```
/// let mut pt = PageTable::new()?;
/// pt.map(VirtAddr(0x1000), PhysAddr(0x80000000), PTE_R | PTE_W | PTE_V)?;
/// ```
pub struct PageTable {
    root_ppn: PhysPageNum,
    walker: PageTableWalker,
    /// Whether this handle owns (and must free) the underlying tree on drop.
    owned: bool,
}

impl Clone for PageTable {
    /// Clone produces a **borrowing** view of the same tree, never a deep copy.
    ///
    /// A deep copy of an address space is done explicitly via `uvmcopy`, not
    /// here. Making `clone` non-owning means a cloned handle can be dropped
    /// freely without freeing the original process's page table.
    fn clone(&self) -> Self {
        Self { root_ppn: self.root_ppn, walker: self.walker, owned: false }
    }
}

impl PageTable {
    /// Create a new empty, **owning** page table.
    ///
    /// Allocates and zeroes the root page table page.
    /// Returns an error if physical memory is exhausted.
    pub fn new() -> Result<Self, &'static str> {
        let root = alloc_page()?;
        // Zero the page
        unsafe { core::ptr::write_bytes(root.to_paddr().0 as *mut u8, 0, PAGE_SIZE) };
        let walker = PageTableWalker::new(root);
        Ok(Self { root_ppn: root, walker, owned: true })
    }

    /// Create a **borrowing** page table view from an existing root page number.
    ///
    /// Does not allocate, and dropping it frees nothing; the caller must ensure
    /// `root_ppn` refers to a live tree owned elsewhere.
    pub fn from_root(root_ppn: PhysPageNum) -> Self {
        Self { root_ppn, walker: PageTableWalker::new(root_ppn), owned: false }
    }
    
    /// Get the root physical page number.
    pub fn root_ppn(&self) -> PhysPageNum { self.root_ppn }
    
    /// Map a virtual page to a physical page.
    /// 
    /// # Arguments
    /// * `vaddr` - Virtual address (must be page-aligned)
    /// * `paddr` - Physical address (must be page-aligned)
    /// * `flags` - PTE flags (R, W, X, U, etc.) - V flag added automatically
    /// 
    /// Returns error if already mapped or allocation fails.
    pub fn map(&mut self, vaddr: VirtAddr, paddr: PhysAddr, flags: u64) -> Result<(), &'static str> {
        let pte = self.walker.walk(vaddr, true).ok_or("walk failed")?;
        if pte.is_valid() { return Err("remap"); }
        pte.set_ppn(PhysPageNum::new(paddr.0 >> 12));
        // Set A (accessed) and D (dirty) up front: this QEMU implements Svade
        // (no hardware A/D update), so a leaf PTE with A=0 faults on first
        // access. Pre-setting them avoids a fault we have no reason to take.
        pte.set_flags(flags | PTE_V | PTE_A | PTE_D);
        Ok(())
    }
    
    /// Unmap a virtual page and free the physical page.
    /// 
    /// Does nothing if the page is not mapped.
    pub fn unmap(&mut self, vaddr: VirtAddr) {
        if let Some(pte) = self.walker.walk(vaddr, false) {
            if pte.is_valid() {
                let ppn = pte.ppn();
                pte.0 = 0;
                free_page(ppn);
            }
        }
    }
    
    /// Translate a virtual address to a physical address.
    /// 
    /// Returns `None` if not mapped or invalid.
    pub fn translate(&self, vaddr: VirtAddr) -> Option<PhysAddr> {
        let pte = self.walker.walk(vaddr, false)?;
        if !pte.is_valid() { return None; }
        let pa = pte.ppn().to_paddr().0 + vaddr.0 % PAGE_SIZE;
        Some(PhysAddr(pa))
    }
    
    /// Deep copy page table entries from another page table.
    /// 
    /// Allocates new physical pages and copies data. Used for fork().
    pub fn copy_from(&mut self, src: &PageTable, size: usize) -> Result<(), &'static str> {
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
    
    /// Activate this page table by writing to `satp` CSR.
    /// 
    /// Also executes `sfence.vma` to flush TLB.
    pub fn activate(&self) {
        crate::arch::asm::w_satp(crate::arch::asm::make_satp(self.root_ppn.0));
        sfence_vma();
    }
}

impl Drop for PageTable {
    /// Recursively free all page table pages and mapped physical pages — but
    /// only for an owning handle. Borrowing views (`owned == false`) free
    /// nothing, so a throwaway view over a live process page table can be
    /// dropped safely.
    fn drop(&mut self) {
        if self.owned {
            self.free_walk(self.root_ppn);
        }
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

use core::sync::atomic::{AtomicPtr, Ordering};

/// Global kernel page table pointer (initialized once).
pub static mut KERNEL_PAGETABLE_PTR: *mut PageTable = core::ptr::null_mut();

/// Get the kernel page table root PPN for `satp`.
pub fn kernel_pagetable() -> PhysPageNum {
    let ptr = unsafe { KERNEL_PAGETABLE_PTR };
    assert!(!ptr.is_null(), "kernel_pagetable not initialized");
    unsafe { (*ptr).root_ppn() }
}

/// Initialize the kernel page table.
/// 
/// Maps: UART, Virtio, PLIC, kernel text (RX), kernel data (RW),
/// trampoline page, and kernel stacks.
pub fn kvminit() {
    let mut pt = Box::new(PageTable::new().expect("kvminit: failed to create kernel page table"));
    map_kernel(&mut pt);
    let ptr = Box::into_raw(pt);
    unsafe { KERNEL_PAGETABLE_PTR = ptr; }
}

fn map_kernel(pt: &mut PageTable) {
    // UART
    pt.map(VirtAddr(0x10000000), PhysAddr(0x10000000), PTE_R | PTE_W).unwrap();
    // VIRTIO
    pt.map(VirtAddr(0x10001000), PhysAddr(0x10001000), PTE_R | PTE_W).unwrap();
    // PLIC - the register block spans ~4MB (priority, pending, enables, and the
    // per-hart threshold/claim windows up to 0x0C200000+), so map the whole range.
    for off in (0..0x400000).step_by(PAGE_SIZE) {
        pt.map(VirtAddr(0x0C000000 + off), PhysAddr(0x0C000000 + off), PTE_R | PTE_W).unwrap();
    }
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
    
    // Map everything from etext up to PHYSTOP as RW: kernel data, bss, the boot
    // stack, the kernel heap, and the physical page pool the frame allocator
    // hands out (page tables, per-process kernel stacks, trapframes, user pages
    // before they are installed). This mirrors xv6's kvmmake and guarantees the
    // kernel can reach any page it allocates once S-mode paging is enabled.
    let _ = end;
    let data_start = (etext + PAGE_SIZE - 1) & !(PAGE_SIZE - 1); // page align up
    let phystop = crate::arch::asm::PHYSTOP;
    let data_pages = (phystop - data_start) / PAGE_SIZE;
    for i in 0..data_pages {
        let addr = data_start + i * PAGE_SIZE;
        pt.map(VirtAddr(addr), PhysAddr(addr), PTE_R | PTE_W).unwrap();
    }

    // Trampoline - map the page containing uservec to TRAMPOLINE virtual address
    let uservec_addr = uservec as usize;
    let trampoline_paddr = PhysAddr((uservec_addr / PAGE_SIZE) * PAGE_SIZE);
    pt.map(VirtAddr(crate::arch::asm::TRAMPOLINE), trampoline_paddr, PTE_R | PTE_X).unwrap();
}

/// Activate the kernel page table on the current hart.
pub fn kvminithart() {
    let ptr = unsafe { KERNEL_PAGETABLE_PTR };
    assert!(!ptr.is_null(), "kernel_pagetable not initialized");
    unsafe { (*ptr).activate(); }
}

/// Create a new user page table with trampoline mapped.
pub fn uvmcreate() -> Result<PageTable, &'static str> {
    let mut pt = PageTable::new()?;
    // Map trampoline - map the page containing uservec to TRAMPOLINE virtual address
    let uservec_addr = uservec as usize;
    let trampoline_paddr = PhysAddr((uservec_addr / PAGE_SIZE) * PAGE_SIZE);
    pt.map(VirtAddr(crate::arch::asm::TRAMPOLINE), trampoline_paddr, PTE_R | PTE_X).unwrap();
    Ok(pt)
}

/// Allocate and map physical pages for user virtual address range.
/// 
/// # Arguments
/// * `pt` - Page table to modify
/// * `old_sz` - Current size of user memory
/// * `new_sz` - New size (must be >= old_sz)
/// 
/// Maps pages with R/W/U/X permissions.
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

/// Free user page table mappings up to size.
pub fn uvmfree(pt: &mut PageTable, sz: usize) {
    for vaddr in (0..sz).step_by(PAGE_SIZE) {
        pt.unmap(VirtAddr(vaddr));
    }
}

/// Copy user page table (for fork).
/// 
/// Deep copies all mapped pages, allocating new physical pages.
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