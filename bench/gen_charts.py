#!/usr/bin/env python3
"""Generate the perf-comparison SVG bar charts embedded in README.md.

Self-contained SVGs with their own light card background so they read on both
GitHub light and dark themes. Run from repo root: python3 bench/gen_charts.py
"""
import os

C_BLUE = "#2563eb"
RUST_RED = "#dc2626"
RUST_GREEN = "#16a34a"
RUST_ORANGE = "#ea580c"
INK = "#1f2937"
MUTE = "#6b7280"
CARD = "#ffffff"
GRID = "#e5e7eb"

FONT = "-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif"


def hbar_chart(title, subtitle, rows, maxval, unit, fmt, ref=None):
    """rows = [(label, value, color)]. ref = (value, text) optional marker."""
    pad_l, pad_r, pad_t = 210, 90, 58
    row_h, gap = 30, 12
    bar_w = 360
    n = len(rows)
    h = pad_t + n * (row_h + gap) + 24
    w = pad_l + bar_w + pad_r
    s = []
    s.append(f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
             f'viewBox="0 0 {w} {h}" font-family="{FONT}">')
    s.append(f'<rect x="0" y="0" width="{w}" height="{h}" rx="10" fill="{CARD}" stroke="{GRID}"/>')
    s.append(f'<text x="20" y="28" font-size="16" font-weight="700" fill="{INK}">{title}</text>')
    if subtitle:
        s.append(f'<text x="20" y="47" font-size="12" fill="{MUTE}">{subtitle}</text>')
    x0 = pad_l
    if ref is not None:
        rv, rt = ref
        rx = x0 + bar_w * (rv / maxval)
        s.append(f'<line x1="{rx:.1f}" y1="{pad_t-6}" x2="{rx:.1f}" y2="{h-14}" '
                 f'stroke="{MUTE}" stroke-width="1" stroke-dasharray="4 3"/>')
        s.append(f'<text x="{rx:.1f}" y="{pad_t-10}" font-size="11" fill="{MUTE}" '
                 f'text-anchor="middle">{rt}</text>')
    y = pad_t
    for label, val, color in rows:
        bw = max(2, bar_w * (val / maxval))
        cy = y + row_h / 2
        s.append(f'<text x="{x0-12}" y="{cy+4:.1f}" font-size="12.5" fill="{INK}" '
                 f'text-anchor="end">{label}</text>')
        s.append(f'<rect x="{x0}" y="{y}" width="{bw:.1f}" height="{row_h}" rx="4" fill="{color}"/>')
        s.append(f'<text x="{x0+bw+8:.1f}" y="{cy+4:.1f}" font-size="12.5" '
                 f'font-weight="600" fill="{INK}">{fmt(val)}{unit}</text>')
        y += row_h + gap
    s.append('</svg>')
    return "\n".join(s)


def main():
    outdir = "docs/img"
    os.makedirs(outdir, exist_ok=True)

    # Chart 1: execution cost, Rust relative to C (C = 100%, shorter = faster).
    rows1 = [
        ("getpid — wall-clock", 84, C_BLUE),
        ("getpid — instructions", 65, RUST_GREEN),
        ("fork — wall-clock", 77, C_BLUE),
        ("fork — instructions", 65, RUST_GREEN),
        ("exec — wall-clock", 47, RUST_GREEN),
    ]
    svg1 = hbar_chart(
        "Rust cost relative to C xv6",
        "shorter = Rust faster · C = 100% (dashed) · -smp 1, QEMU 8.2.2",
        rows1, maxval=110, unit="%", fmt=lambda v: f"{v}", ref=(100, "C = 100%"))
    with open(f"{outdir}/perf-exec.svg", "w") as f:
        f.write(svg1)

    # Chart 2: forkbench user binary .text size.
    rows2 = [
        ("C xv6", 2365, C_BLUE),
        ("Rust — default", 12498, RUST_RED),
        ("Rust — build-std slim", 4946, RUST_ORANGE),
        ("Rust — source slim", 1915, RUST_GREEN),
    ]
    svg2 = hbar_chart(
        "forkbench user binary size (.text)",
        "bloat is core::fmt + alloc, not Rust — removable",
        rows2, maxval=13000, unit="", fmt=lambda v: f"{v:,}")
    with open(f"{outdir}/perf-size.svg", "w") as f:
        f.write(svg2)

    print(f"wrote {outdir}/perf-exec.svg and {outdir}/perf-size.svg")


if __name__ == "__main__":
    main()
