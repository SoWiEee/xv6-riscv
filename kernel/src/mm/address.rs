// kernel/src/mm/address.rs
//! Typed address wrappers for physical and virtual memory addresses.
//!
//! These newtype wrappers provide compile-time safety by preventing accidental
//! mixing of physical addresses, virtual addresses, physical page numbers,
//! and virtual page numbers. All are 64-bit values on RISC-V 64-bit.

use core::ops::{Add, Sub, AddAssign, SubAssign};

/// Physical memory address.
/// 
/// Represents a byte address in physical memory. Created from the frame allocator
/// or by converting from `PhysPageNum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PhysAddr(pub usize);

/// Virtual memory address.
/// 
/// Represents a byte address in virtual memory (kernel or user space).
/// Created from page table operations or by converting from `VirtPageNum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct VirtAddr(pub usize);

/// Physical page number.
/// 
/// Represents a 4KB physical page frame number. Used for page table entries
/// and frame allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PhysPageNum(pub usize);

/// Virtual page number.
/// 
/// Represents a 4KB virtual page number. Used for page table walks
/// and virtual memory management.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct VirtPageNum(pub usize);

impl PhysAddr {
    /// Create a new physical address from a raw usize.
    pub const fn new(addr: usize) -> Self { Self(addr) }

    /// Round down to the containing page number.
    pub fn floor(&self) -> PhysPageNum { PhysPageNum(self.0 / 4096) }

    /// Round up to the next page number.
    pub fn ceil(&self) -> PhysPageNum { PhysPageNum((self.0 + 4095) / 4096) }

    /// Get the offset within the page (0-4095).
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl VirtAddr {
    /// Create a new virtual address from a raw usize.
    pub const fn new(addr: usize) -> Self { Self(addr) }

    /// Round down to the containing page number.
    pub fn floor(&self) -> VirtPageNum { VirtPageNum(self.0 / 4096) }

    /// Round up to the next page number.
    pub fn ceil(&self) -> VirtPageNum { VirtPageNum((self.0 + 4095) / 4096) }

    /// Get the offset within the page (0-4095).
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl PhysPageNum {
    /// Create a new physical page number from a raw usize.
    pub const fn new(ppn: usize) -> Self { Self(ppn) }

    /// Convert to the base physical address of this page.
    pub fn to_paddr(&self) -> PhysAddr { PhysAddr(self.0 * 4096) }

    /// Convert to the base virtual address (for identity mapping).
    pub fn to_vaddr(&self) -> VirtAddr { VirtAddr(self.0 * 4096) }
}

impl VirtPageNum {
    /// Create a new virtual page number from a raw usize.
    pub const fn new(vpn: usize) -> Self { Self(vpn) }

    /// Convert to the base virtual address of this page.
    pub fn to_vaddr(&self) -> VirtAddr { VirtAddr(self.0 * 4096) }
}

macro_rules! impl_arith {
    ($t:ty) => {
        impl Add<usize> for $t {
            type Output = Self;
            fn add(self, rhs: usize) -> Self { Self(self.0 + rhs) }
        }
        impl Sub<usize> for $t {
            type Output = Self;
            fn sub(self, rhs: usize) -> Self { Self(self.0 - rhs) }
        }
        impl AddAssign<usize> for $t {
            fn add_assign(&mut self, rhs: usize) { self.0 += rhs; }
        }
        impl SubAssign<usize> for $t {
            fn sub_assign(&mut self, rhs: usize) { self.0 -= rhs; }
        }
    };
}

impl_arith!(PhysAddr);
impl_arith!(VirtAddr);
impl_arith!(PhysPageNum);
impl_arith!(VirtPageNum);

impl Sub for PhysAddr { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for VirtAddr { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for PhysPageNum { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }
impl Sub for VirtPageNum { type Output = usize; fn sub(self, rhs: Self) -> usize { self.0 - rhs.0 } }