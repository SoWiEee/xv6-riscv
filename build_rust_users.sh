#!/bin/bash
set -e

# Build Rust user programs (release for smaller size)
echo "Building Rust user programs..."
cargo build --release -p xv6-user

# Copy binaries to user directory with _ prefix (as mkfs expects)
echo "Copying binaries..."
cp target/riscv64imac-unknown-none-elf/release/sh user/_sh
cp target/riscv64imac-unknown-none-elf/release/ls user/_ls
cp target/riscv64imac-unknown-none-elf/release/cat user/_cat
cp target/riscv64imac-unknown-none-elf/release/init user/_init

# Create fs.img with only the programs that exist
echo "Creating fs.img..."
./mkfs/mkfs fs.img README \
    user/_cat \
    user/_echo \
    user/_forktest \
    user/_grep \
    user/_init \
    user/_kill \
    user/_ln \
    user/_ls \
    user/_mkdir \
    user/_rm \
    user/_sh \
    user/_stressfs \
    user/_usertests \
    user/_grind \
    user/_wc \
    user/_zombie \
    user/_logstress \
    user/_forphan \
    user/_dorphan \
    user/_sync \
    user/_test_leak_poc

echo "Done!"