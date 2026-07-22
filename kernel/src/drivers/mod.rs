// kernel/src/drivers/mod.rs
pub mod uart;
pub mod virtio;
pub mod console;

pub fn uart_intr() {
    uart::uart_intr();
}

pub fn virtio_intr() {
    virtio::virtio_intr();
}

pub fn virtio_init() {
    virtio::virtio_init();
}

pub fn console_init() {
    console::console_init();
}