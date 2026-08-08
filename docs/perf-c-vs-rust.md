# Performance: C xv6 vs the Rust rewrite

A first, deliberately narrow comparison between the original C kernel (root
`Makefile` build, compiled `-O`) and the Rust rewrite (`rust-rewrite`, release
`-O3`-level). Measured on QEMU 8.2.2 `-machine virt`, `-smp 1`, same host.

> **This compares two implementations, not two languages.** Two workloads: a
> tight `getpid()` loop (cheapest syscall — isolates trap entry/dispatch/exit)
> and a `fork()`+`wait()` loop (a heavy path — address-space copy, proc alloc,
> scheduler round-trips, `uvmfree`).

## Benchmark programs

- **syscall latency:** `user/syscallbench.c` + `user/src/bin/syscallbench.rs` —
  read `N` from argv, bracket an `N`-iteration `getpid()` loop with `uptime()`,
  print `SYSCALLBENCH n=<N> ticks=<Δ> acc=<fold>`. `acc` folds every return so
  the loop can't be elided.
- **fork heavy path:** `user/forkbench.c` + `user/src/bin/forkbench.rs` —
  `N` iterations of `fork()` then `wait()`, child `exit(0)`s immediately; print
  `FORKBENCH n=<N> ticks=<Δ>`.

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

### Heavy path — `fork()` + `wait()`

Difference method, `N` = 2000/4000. Host = median of 5; icount = deterministic
(Rust identical across runs; C's two glitch tick reads discarded by the median).

| Kernel | host µs / fork | icount ticks / fork | ≈ instructions / fork* |
|--------|---------------:|--------------------:|-----------------------:|
| C      | 949            | 0.0115              | ~1.15M                 |
| Rust   | 731            | 0.0075              | ~0.75M                 |

Rust `fork`+`wait` is ~23% faster (0.77×) and ~35% fewer instructions (0.65×) —
the same instruction ratio as `getpid`. Notably, the Rust `forkbench` image is
larger (text 12,498 vs 2,365 → a few more pages for `uvmcopy` to copy per
`fork`), yet Rust still wins: `fork` cost is dominated by fixed overhead (proc
alloc, two scheduler round-trips, trap save/restore, exit/reap), and Rust's path
is leaner. The address-space port (compact `sz`) means `fork`/`exit` are *not* a
Rust bottleneck.

### Heavy path — `fork()` + `exec("nop")` + `wait()`

`execbench` adds an `exec` of a do-nothing `nop` program (`user/nop.c` +
`user/src/bin/nop.rs`) to the fork loop; the marginal `exec` cost is
`execbench − forkbench`. Difference method, `N` = 500/1000, host = median of 5.

| Kernel | host µs / iter (fork+exec+wait) | isolated exec (− fork) |
|--------|--------------------------------:|-----------------------:|
| C      | 3952                            | ~3003 µs               |
| Rust   | 1845                            | ~1114 µs               |

Rust `fork`+`exec`+`wait` is ~2.1× faster; isolated `exec` ~2.7× faster. (The
icount pass had a harness read glitch on C's tick values — the regex now
requires a trailing newline — so the wall-clock numbers are the reliable ones
here.) So even the FS-touching `exec` path is not a Rust weakness.

### (C) Static size

| Metric                     | C      | Rust    | Ratio |
|----------------------------|-------:|--------:|------:|
| Kernel `.text`             | 30,760 | 131,072 | ~3.7× |
| Kernel source lines        | 6,271  | 8,017   | 1.28× |
| user `syscallbench` `.text`| 2,343  | 12,448  | 5.3×  |
| user `forkbench` `.text`   | 2,365  | 12,498  | 5.3×  |

Rust kernel `.text` includes ~16.7 KB of `include_bytes!`-embedded `init`.
Kernel `.bss` (C 103 KB vs Rust 16.9 MB) is **not** comparable: Rust reserves a
16 MB static `HEAP` array plus the frame free-list in `.bss`, whereas C xv6
uses physical RAM directly as its `kalloc` pool.

### Why the Rust user binaries are 5× larger — and how to shrink them

The bloat is **not** intrinsic to Rust. A do-nothing Rust program (`nop.rs`,
just `exit(0)`) is **32 bytes** of `.text` — smaller than the C equivalent. The
5× gap comes entirely from two things the ordinary programs pull in:

- **`core::fmt`** via `println!` — `Formatter::pad_integral`, `fmt::write`, the
  `Display` impls, `do_count_chars`. This is the single biggest chunk.
- **`alloc`** via `args()` (returns a `Vec<&str>`) and `init_heap()` — the
  `linked_list_allocator` and `RawVec`.

Things that do **not** help: `opt-level = "z"` and `codegen-units = 1` both made
the binary *larger* here (the default `opt-level = 3` + `lto = true` already
inlines aggressively; `"z"` disables size-relevant inlining). `panic = "abort"`
and `lto` are already set.

What does help:

- **Avoid `core::fmt` / `alloc` in the program.** `forkbench_slim.rs` does the
  identical work but hand-rolls integer→ASCII output over `syscall::write` and
  parses `argv[1]` manually (no `println!`, no `args()`, no `init_heap`). Its
  `.text` is **1,915 bytes vs forkbench's 12,498** — a 6.5× drop, *below* the C
  forkbench's 2,365.

  | binary                | `.text` |
  |-----------------------|--------:|
  | `forkbench` (fmt+alloc) | 12,498 |
  | `forkbench_slim`        |  1,915 |
  | C `forkbench`           |  2,365 |
  | `nop`                   |     32 |

- **Project-wide (now applied):** `build_rust_users.sh` builds the user crate
  with `-Z build-std=core,alloc,compiler_builtins` and `panic=immediate-abort`
  (needs `rustup component add rust-src`). This rebuilds `core`/`alloc` without
  the panic-formatting machinery — no source changes — and roughly halves every
  binary (forkbench 12,498 → 4,946, cat 12,740 → 4,064, ls 14,004 → 6,780,
  sh 21,141 → 12,124). usertests still passes (17/17). The kernel keeps normal
  `panic` so its debug messages survive. `immediate-abort` was formerly the
  `panic_immediate_abort` build-std feature; on current nightly it is the
  `immediate-abort` panic strategy (`-Z unstable-options -C panic=immediate-abort`).
  This is complementary to the source-level slim above (build-std removes
  panic's fmt; the source slim also removes `println!`'s).

Smaller user images also mean a smaller `sz`, so `fork`'s `uvmcopy` copies fewer
pages — the technique directly improves the `fork` numbers above.

## Interpretation

On the minimal syscall path the Rust rewrite is *leaner* — a tighter
trap/dispatch path plus `-O3` (vs C xv6's `-O`). The trade-off is a much larger
static footprint: monomorphization and the formatting / panic / allocator
machinery are pulled in even for a trivial program.

## Caveats

- **`getpid` and `fork` only.** These cover the trap/dispatch path and a
  heavy proc/address-space path. FS-heavy workloads (`exec`, file I/O) add ELF
  loading and disk I/O and are not covered here. The Rust port uses xv6's
  compact address-space layout (`sz` = program + guard + stack), so
  `uvmcopy`/`uvmfree` walk only the small mapped range on `fork`/`exit`, not a
  ~2 GB gap; `Inode::lock` is a real sleeplock, not a stub.
- **`fork` fairness.** Rust's `forkbench` image is ~5× larger, so its `fork`
  copies a few more pages; Rust is still faster, so fixed overhead dominates.
- Wall-clock under QEMU is emulation-bound; the icount metric is the
  emulation-independent one.

## Kernel optimizations

The FS write path was dominated by virtio round-trips (each ~2.5 ms of QEMU poll
latency). Three landed changes cut them, each verified with `user/fsbench`
(N 1 KiB writes, difference method, -smp 1) and usertests 17/17 on -smp 1 and
-smp 3:

- **Batched disk I/O** (`read_block`/`write_block`): a 1024-byte FS block is two
  consecutive 512-byte sectors, and virtio-blk transfers `buffer_len / 512`
  sectors per request, so one 1024-byte request moves the whole block instead of
  two per-sector round-trips. **31,556 → 24,777 µs per block write, ~21%.**
- **Batched log write** (`write_log`): a transaction's N log blocks are
  consecutive on disk, so they are gathered into one contiguous buffer and
  flushed in a single request (via the generalized `virtio_rw_buf`) instead of
  one `bwrite` per block. **~5.7%.** (`install_trans`'s home-block writes go to
  *scattered* sectors, so they can't be collapsed the same way.)
- **Skip the read when zeroing a freshly allocated block** (`balloc`):
  `bget_zeroed` grabs the buffer without reading its stale contents from disk
  (they're about to be overwritten with zeros). **~3.6%** (correct by
  construction; at the noise edge).

Cumulatively FS writes went ~31,556 → ~23,557 µs per block (~25%).

**Tried and reverted:** (a) kernel fat-LTO + `codegen-units=1` — no measurable
change; `getpid`/`fork` are dominated by the assembly trap path (register
save/restore, the trampoline `satp` switch, the `sfence.vma` TLB flush), not
Rust call overhead. (b) `bget_zeroed` for the log block in `write_log` — the log
blocks are cached across commits, so it just added a pointless memset. Wall-clock
noise (~4–7%) and coarse icount ticks put sub-noise micro-optimizations below the
measurement floor.

## Reproduce

```bash
# C image (Makefile builds all UPROGS incl. _syscallbench):
make fs.img && cp fs.img fs_c.img

# Rust image:
cargo build --release --target riscv64imac-unknown-none-elf -p xv6-user
for b in init sh cat echo ls syscallbench forkbench execbench nop; do \
  cp target/riscv64imac-unknown-none-elf/release/$b user/_$b; done
./mkfs/mkfs fs_rust.img README user/_init user/_sh user/_cat user/_echo user/_ls \
  user/_syscallbench user/_forkbench user/_execbench user/_nop

# Measure (difference method + optional --icount for deterministic ticks):
RK=target/riscv64imac-unknown-none-elf/release/xv6-kernel
python3 bench/syscallbench_harness.py --kernel kernel/kernel --image fs_c.img --n 1000000
python3 bench/syscallbench_harness.py --kernel $RK --image fs_rust.img --n 1000000
# Heavy paths (--prog forkbench / execbench):
python3 bench/syscallbench_harness.py --kernel $RK --image fs_rust.img --prog forkbench --n 2000
python3 bench/syscallbench_harness.py --kernel $RK --image fs_rust.img --prog execbench --n 500
```
