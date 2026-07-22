// kernel/src/mm/address.rs
use core::ops::{Add, Sub, AddAssign, SubAssign};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PhysAddr(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct VirtAddr(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct PhysPageNum(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct VirtPageNum(pub usize);

impl PhysAddr {
    pub const fn new(addr: usize) -> Self { Self(addr) }
    pub fn floor(&self) -> PhysPageNum { PhysPageNum(self.0 / 4096) }
    pub fn ceil(&self) -> PhysPageNum { PhysPageNum((self.0 + 4095) / 4096) }
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl VirtAddr {
    pub const fn new(addr: usize) -> Self { Self(addr) }
    pub fn floor(&self) -> VirtPageNum { VirtPageNum(self.0 / 4096) }
    pub fn ceil(&self) -> VirtPageNum { VirtPageNum((self.0 + 4095) / 4096) }
    pub fn page_offset(&self) -> usize { self.0 % 4096 }
}

impl PhysPageNum {
    pub const fn new(ppn: usize) -> Self { Self(ppn) }
    pub fn to_paddr(&self) -> PhysAddr { PhysAddr(self.0 * 4096) }
    pub fn to_vaddr(&self) -> VirtAddr { VirtAddr(self.0 * 4096) }
}

impl VirtPageNum {
    pub const fn new(vpn: usize) -> Self { Self(vpn) }
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