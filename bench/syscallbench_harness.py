#!/usr/bin/env python3
"""Drive syscallbench under QEMU and report host wall-clock + guest ticks.

Boots the given kernel/image on -smp 1, waits for the sh prompt, runs
`syscallbench N`, and times send -> SYSCALLBENCH marker. Use two N values and
difference them (T(2N)-T(N)) to cancel exec/boot fixed cost. Pass --icount for
deterministic, instruction-proportional guest ticks.

Requires pexpect (`pip install pexpect`).
"""
import argparse
import statistics
import time

import pexpect


def run_once(kernel, image, n, smp, icount, timeout, PROG):
    cmd = [
        "qemu-system-riscv64", "-machine", "virt", "-bios", "none",
        "-kernel", kernel, "-m", "128M", "-smp", str(smp), "-nographic",
        "-global", "virtio-mmio.force-legacy=false",
        "-drive", f"file={image},if=none,format=raw,id=x0",
        "-device", "virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0",
    ]
    if icount:
        cmd += ["-icount", "shift=0"]
    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", timeout=timeout)
    try:
        child.expect(r"\$ ")
        time.sleep(0.2)
        t0 = time.monotonic()
        child.sendline(f"{PROG} {n}")
        # Require a non-digit (space before an ` acc=` suffix, or the line's
        # newline) after the tick digits so pexpect waits for the whole number —
        # a buffer split mid-number would otherwise capture a truncated value.
        child.expect(rf"{PROG.upper()} n=(\d+) ticks=(\d+)[ \r\n]")
        host_s = time.monotonic() - t0
        ticks = int(child.match.group(2))
        return host_s, ticks
    finally:
        try:
            child.terminate(force=True)
        except Exception:
            pass


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--kernel", required=True, help="path to the kernel ELF")
    ap.add_argument("--image", required=True, help="path to the fs.img")
    ap.add_argument("--prog", default="syscallbench",
                    help="guest program to run; marker is its uppercase name")
    ap.add_argument("--n", type=int, default=1_000_000, help="base iteration count N")
    ap.add_argument("--reps", type=int, default=5, help="repetitions per N")
    ap.add_argument("--smp", type=int, default=1)
    ap.add_argument("--icount", action="store_true", help="run with -icount shift=0")
    ap.add_argument("--timeout", type=int, default=600)
    args = ap.parse_args()

    metric = "ticks" if args.icount else "host_s"
    print(f"kernel={args.kernel} image={args.image} icount={args.icount} metric={metric}")
    medians = {}
    for n in (args.n, 2 * args.n):
        vals = []
        for r in range(args.reps):
            host_s, ticks = run_once(args.kernel, args.image, n, args.smp, args.icount, args.timeout, args.prog)
            v = ticks if args.icount else host_s
            vals.append(v)
            print(f"  N={n:>10} rep{r} host_s={host_s:.4f} ticks={ticks}")
        medians[n] = statistics.median(vals)
    marginal = (medians[2 * args.n] - medians[args.n]) / args.n
    unit = "ticks/syscall" if args.icount else "s/syscall"
    print(f"marginal per-syscall = {marginal:.3e} {unit}"
          + ("" if args.icount else f"  ({marginal * 1e6:.4f} us/syscall)"))


if __name__ == "__main__":
    main()
