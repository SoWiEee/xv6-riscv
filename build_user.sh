#!/bin/bash
set -e

# Build user programs and create fs.img for xv6-riscv

echo "Building kernel..."
make kernel/kernel

echo "Building user programs..."
make user/_init user/_sh user/_ls user/_cat user/_echo user/_kill user/_ln user/_mkdir user/_rm user/_stressfs user/_usertests user/_grind user/_wc user/_zombie user/_logstress user/_forphan user/_dorphan user/_sync user/_cat user/_echo user/_forktest user/_grep

echo "Creating fs.img with mkfs..."
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

echo "Done! fs.img created successfully."
echo "Run with: make qemu"
