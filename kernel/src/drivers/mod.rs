// kernel/src/drivers/mod.rs
pub mod uart;
pub mod virtio;

pub fn uart_intr() {
    uart::uart_intr();
}

pub fn virtio_intr() {
    virtio::virtio_intr();
}

pub fn virtio_init() {
    virtio::virtio_init();
}