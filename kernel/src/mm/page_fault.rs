// kernel/src/mm/page_fault.rs
use super::address::VirtAddr;
use crate::proc::current_process;
use crate::mm::page_table::PageTable;
use crate::mm::frame_allocator::alloc_page;

pub fn handle_page_fault(pagetable: &mut PageTable, vaddr: usize, _read: bool) -> Result<(), &'static str> {
    let p = current_process();
    let vaddr = VirtAddr::new(vaddr);

    let sz = p.sz();

    if vaddr.0 >= sz {
        return Err("invalid address");
    }

    if pagetable.translate(vaddr).is_some() {
        return Err("already mapped");
    }
    
    let ppn = alloc_page().map_err(|_| "out of memory")?;
    // Demand-allocated pages (lazy stack/heap growth) are ordinary user data —
    // always readable AND writable. The previous logic keyed PTE_W on `read`,
    // which produced a NON-writable page for a store fault (so the faulting
    // store re-faulted forever / was killed) and a writable page only for
    // loads — exactly backwards. Frames from the allocator are not zeroed, so
    // zero the page too (demand-zero semantics; garbage would otherwise leak).
    unsafe {
        core::ptr::write_bytes(ppn.to_paddr().0 as *mut u8, 0, crate::arch::paging::PAGE_SIZE);
    }
    let flags = crate::arch::paging::PTE_R | crate::arch::paging::PTE_W | crate::arch::paging::PTE_U;
    pagetable.map(vaddr, ppn.to_paddr(), flags)?;

    Ok(())
}