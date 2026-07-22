// kernel/src/mm/page_fault.rs
use super::address::VirtAddr;
use crate::proc::current_process;
use crate::mm::page_table::PageTable;
use crate::mm::frame_allocator::alloc_page;

pub fn handle_page_fault(pagetable: &mut PageTable, vaddr: usize, read: bool) -> Result<(), &'static str> {
    let p = current_process();
    let vaddr = VirtAddr::new(vaddr);
    
    if vaddr.0 >= p.sz {
        return Err("invalid address");
    }
    
    let vpn = vaddr.floor();
    if pagetable.translate(vaddr).is_some() {
        return Err("already mapped");
    }
    
    let ppn = alloc_page().map_err(|_| "out of memory")?;
    let flags = crate::arch::paging::PTE_R | crate::arch::paging::PTE_U | 
                if read { crate::arch::paging::PTE_W } else { 0 };
    pagetable.map(vaddr, ppn.to_paddr(), flags)?;
    
    Ok(())
}