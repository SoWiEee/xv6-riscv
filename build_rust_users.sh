#!/bin/bash
set -e

# Build Rust user programs (release for smaller size).
#
# build-std rebuilds core/alloc with panic=immediate-abort, which strips the
# panic-formatting machinery that otherwise bloats every user binary (~2.5x
# smaller .text — e.g. forkbench 12,498 -> 4,946). This applies ONLY to the
# user build; the kernel (built below) keeps normal panic so its panic
# messages survive for debugging.
#
# RUSTFLAGS must replicate .cargo/config.toml's target flags because setting
# RUSTFLAGS replaces (does not merge with) the config's rustflags; panic is
# switched from abort to immediate-abort. Keep this list in sync with
# .cargo/config.toml [target.riscv64imac-unknown-none-elf].rustflags.
echo "Building Rust user programs (slim: build-std + panic=immediate-abort)..."
RUSTFLAGS="-C link-arg=-nostdlib -C link-arg=-static -C target-feature=+reserve-x4 -Z unstable-options -C panic=immediate-abort" \
  cargo build --release --target riscv64imac-unknown-none-elf -p xv6-user \
  -Z build-std=core,alloc,compiler_builtins

# Copy binaries to user directory with _ prefix (as mkfs expects).
# These are the programs that have been ported to Rust; the remaining
# user/_* entries below are still the original C xv6 binaries.
echo "Copying binaries..."
cp target/riscv64imac-unknown-none-elf/release/sh user/_sh
cp target/riscv64imac-unknown-none-elf/release/ls user/_ls
cp target/riscv64imac-unknown-none-elf/release/cat user/_cat
cp target/riscv64imac-unknown-none-elf/release/init user/_init
cp target/riscv64imac-unknown-none-elf/release/echo user/_echo
cp target/riscv64imac-unknown-none-elf/release/mkdir user/_mkdir
cp target/riscv64imac-unknown-none-elf/release/rm user/_rm
cp target/riscv64imac-unknown-none-elf/release/grep user/_grep
cp target/riscv64imac-unknown-none-elf/release/wc user/_wc
cp target/riscv64imac-unknown-none-elf/release/kill user/_kill
cp target/riscv64imac-unknown-none-elf/release/ln user/_ln
cp target/riscv64imac-unknown-none-elf/release/zombie user/_zombie
cp target/riscv64imac-unknown-none-elf/release/forktest user/_forktest
cp target/riscv64imac-unknown-none-elf/release/stressfs user/_stressfs
cp target/riscv64imac-unknown-none-elf/release/usertests user/_usertests
cp target/riscv64imac-unknown-none-elf/release/grind user/_grind
cp target/riscv64imac-unknown-none-elf/release/logstress user/_logstress
cp target/riscv64imac-unknown-none-elf/release/sync user/_sync
cp target/riscv64imac-unknown-none-elf/release/forphan user/_forphan
cp target/riscv64imac-unknown-none-elf/release/dorphan user/_dorphan

# Rebuild the kernel AFTER refreshing user/_init: the first process (userinit)
# runs an init image embedded into the kernel at compile time via
# include_bytes!("../../user/_init"), so a stale kernel would boot the old init.
echo "Rebuilding kernel to embed the fresh init..."
cargo build --release --target riscv64imac-unknown-none-elf -p xv6-kernel

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