# Performance: C xv6 vs the Rust rewrite

A first, deliberately narrow comparison between the original C kernel (root
`Makefile` build, compiled `-O`) and the Rust rewrite (`rust-rewrite`, release
`-O3`-level). Measured on QEMU 8.2.2 `-machine virt`, `-smp 1`, same host.

> **This compares two implementations, not two languages.** The workload is a
> tight `getpid()` loop — the cheapest syscall — so it isolates the trap
> entry / dispatch / exit path. It says nothing about `fork`/`exec`/FS, where
> the Rust port is currently *slower* by construction (see
> [Caveats](#caveats)).

## Benchmark program

`user/syscallbench.c` and `user/src/bin/syscallbench.rs` are kept structurally
identical: read `N` from argv, bracket an `N`-iteration `getpid()` loop with
`uptime()`, print `SYSCALLBENCH n=<N> ticks=<Δ> acc=<fold>`. `acc` folds every
return so the loop can't be elided.

The host harness (`bench/syscallbench_harness.py`) boots QEMU, waits for the
`$ ` shell prompt, runs `syscallbench N`, and times send → marker.

**Difference method.** A single run's time is dominated by `exec` (loading the
program), not the loop. Running `N` and `2N` and taking `T(2N) − T(N)` cancels
`exec` and every other fixed cost, leaving the marginal per-syscall cost.

## Results

### (A) Syscall latency — host wall-clock

`N` = 1M and 2M, median of 5 runs:

| Kernel | host_s @1M | host_s @2M | marginal / syscall |
|--------|-----------:|-----------:|-------------------:|
| C      | 18.40 s    | 36.99 s    | **18.59 µs**       |
| Rust   | 16.01 s    | 31.69 s    | **15.68 µs**       |

Rust is ~16% faster per `getpid` (0.84×).

### (B) Instruction count — `-icount shift=0` (deterministic)

Under `-icount shift=0`, guest time advances with instructions, so the
`uptime`-tick delta is deterministic and instruction-proportional. `N` = 5M and
10M, 3 runs each — ticks were identical across runs (C: 53/53/53, 105/105/105;
Rust: 34/34/34, 68/68/69), confirming determinism.

| Kernel | ticks / 1e6 syscalls | ≈ instructions / getpid* |
|--------|---------------------:|-------------------------:|
| C      | 10.4                 | ~1040                    |
| Rust   | 6.8                  | ~680                     |

Rust executes ~35% fewer instructions per `getpid` (0.65×).

<sub>*Instruction estimate uses mtime @ 10 MHz under icount (≈1e8 instructions
per software tick); the absolute number is rough — the 0.65 ratio is the robust
result.</sub>

### (C) Static size

| Metric                     | C      | Rust    | Ratio |
|----------------------------|-------:|--------:|------:|
| Kernel `.text`             | 30,760 | 131,072 | ~3.7× |
| Kernel source lines        | 6,271  | 8,017   | 1.28× |
| user `syscallbench` `.text`| 2,343  | 12,448  | 5.3×  |

Rust kernel `.text` includes ~16.7 KB of `include_bytes!`-embedded `init`.
Kernel `.bss` (C 103 KB vs Rust 16.9 MB) is **not** comparable: Rust reserves a
16 MB static `HEAP` array plus the frame free-list in `.bss`, whereas C xv6
uses physical RAM directly as its `kalloc` pool.

## Interpretation

On the minimal syscall path the Rust rewrite is *leaner* — a tighter
trap/dispatch path plus `-O3` (vs C xv6's `-O`). The trade-off is a much larger
static footprint: monomorphization and the formatting / panic / allocator
machinery are pulled in even for a trivial program.

## Caveats

- **`getpid` only.** This isolates the trap / dispatch / exit path. Heavier
  paths (`fork`/`exec`/FS) also involve ELF loading, page copying, and disk
  I/O, and were left out to keep the comparison clean — not because they are
  known-slow. (The Rust port already uses xv6's compact address-space layout:
  `sz` is program + guard + stack, so `uvmcopy`/`uvmfree` walk only the small
  mapped range on `fork`/`exit`, not a ~2 GB gap. `Inode::lock` is a real
  sleeplock, not a stub.)
- Wall-clock under QEMU is emulation-bound; the icount metric is the
  emulation-independent one.

## Reproduce

```bash
# C image (Makefile builds all UPROGS incl. _syscallbench):
make fs.img && cp fs.img fs_c.img

# Rust image:
cargo build --release --target riscv64imac-unknown-none-elf -p xv6-user
for b in init sh cat echo ls syscallbench; do \
  cp target/riscv64imac-unknown-none-elf/release/$b user/_$b; done
./mkfs/mkfs fs_rust.img README user/_init user/_sh user/_cat user/_echo user/_ls user/_syscallbench

# Measure (difference method + icount):
python3 bench/syscallbench_harness.py --kernel kernel/kernel --image fs_c.img --n 1000000
python3 bench/syscallbench_harness.py --kernel target/riscv64imac-unknown-none-elf/release/xv6-kernel \
        --image fs_rust.img --n 1000000
```
