# xv6-riscv Rust Rewrite

A complete rewrite of the xv6-riscv teaching operating system kernel in **Rust**, leveraging Rust's memory safety, ownership model, and zero-cost abstractions to eliminate C memory bugs while maintaining identical behavior and performance characteristics.

## Overview

This project is a faithful Rust reimplementation of [xv6-riscv](https://github.com/mit-pdos/xv6-riscv), the RISC-V port of MIT's xv6 teaching operating system. The original xv6 was written in ANSI C; this version uses Rust's powerful type system to provide compile-time guarantees against common kernel bugs like use-after-free, double-free, buffer overflows, and data races.

### Why Rust?

| Feature | Benefit for Kernel Development |
|---------|--------------------------------|
| **No runtime/GC** | Direct hardware control, predictable latency |
| **Borrow checker** | Compile-time prevention of memory safety bugs |
| **Ownership model** | Clear resource lifetime management |
| **Zero-cost abstractions** | Performance matching C |
| **Type safety** | Typed addresses (`PhysAddr`, `VirtAddr`) prevent mixing |
| **Rich ecosystem** | `riscv`, `tock-registers`, `embedded-hal` crates |

### Compatibility

- **Binary compatible**: Same syscall numbers, same ELF format for user programs
- **Disk compatible**: Same FS layout (can mount C xv6 `fs.img`)
- **Test compatible**: All existing usertests pass unchanged

## Quick Start

### Prerequisites

- Rust toolchain (1.75+): `rustup target add riscv64gc-unknown-none-elf`
- RISC-V toolchain: `riscv64-unknown-elf-gcc` (for user programs)
- QEMU: `qemu-system-riscv64` (virt machine)
- `ld.lld` linker

### Building

```bash
# Build kernel and user programs
cargo build --release

# Or use the provided Makefile
make
```

### Running

```bash
# Run in QEMU
cargo run --release

# Or with Makefile
make qemu
```

### Testing

```bash
# Run unit tests
cargo test

# Run integration tests (requires QEMU)
cargo test --test integration
```

## Architecture

See [docs/architecture.md](docs/architecture.md) for a detailed architecture overview.

## Migration from C xv6

See [docs/migration.md](docs/migration.md) for a guide on porting C xv6 code to Rust.

## Project Structure

```
xv6-rust/
├── kernel/                 # Kernel crate (no_std)
│   ├── src/
│   │   ├── arch/           # RISC-V architecture specifics
│   │   ├── mm/             # Memory management
│   │   ├── fs/             # File system
│   │   ├── proc/           # Process management
│   │   ├── sync/           # Synchronization primitives
│   │   ├── drivers/        # Device drivers
│   │   ├── syscall/        # System call handling
│   │   ├── trap/           # Trap handling
│   │   └── lib.rs          # Crate root
│   ├── Cargo.toml
│   └── memory.x            # Linker script
├── user/                   # User-space programs (std)
│   ├── src/bin/            # Each program as binary (sh, ls, cat, etc.)
│   ├── src/lib.rs          # User library (ulib equivalent)
│   └── Cargo.toml
├── user-lib/               # Shared user library
├── xtask/                  # Build automation
├── Cargo.toml              # Workspace root
└── docs/
    ├── architecture.md     # Architecture documentation
    └── migration.md        # Migration guide
```

## License

MIT License - see [LICENSE](LICENSE) for details.

Based on xv6-riscv by MIT (Frans Kaashoek, Robert Morris, et al.)