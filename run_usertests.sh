#!/bin/bash
# run_usertests.sh - Run xv6-rust usertests

set -e

echo "Building kernel..."
cargo build --release --target riscv64imac-unknown-none-elf -p xv6-kernel

echo "Building user programs..."
cargo build --release --target riscv64imac-unknown-none-elf -p xv6-user

echo "Creating filesystem image..."
./build_rust_users.sh

echo "Running kernel with usertests..."
KERNEL=target/riscv64imac-unknown-none-elf/release/xv6-kernel
qemu-system-riscv64 -machine virt -bios none -kernel $KERNEL -m 128M -smp 1 -nographic \
  -global virtio-mmio.force-legacy=false \
  -drive file=fs.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0