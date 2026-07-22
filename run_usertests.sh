#!/bin/bash
# run_usertests.sh - Run xv6-rust usertests

set -e

echo "Building kernel..."
cargo build --release -p xv6-kernel

echo "Building user programs..."
cargo build --release -p xv6-user

echo "Creating filesystem image..."
./build_rust_users.sh

echo "Running kernel with usertests..."
cargo run --target riscv64imac-unknown-none-elf -p xv6-kernel -- usertests