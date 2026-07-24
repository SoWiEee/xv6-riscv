# Architecture Documentation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the stale architecture overview with an evidence-based English map of the in-progress Rust xv6 migration and its C xv6 reference implementation.

**Architecture:** `docs/architecture.md` becomes the sole reader-facing deliverable. It will start with the repository's two implementation tracks, then describe each Rust subsystem alongside its C xv6 source counterpart and its evidence-supported migration status. Build and validation instructions will keep the C reference Makefile path separate from the Rust Cargo/script path.

**Tech Stack:** Markdown; Rust Cargo manifests and sources; C xv6 sources; GNU Make.

## Global Constraints

- Keep the document in English.
- Describe the Rust code as an in-progress migration, not a completed or binary-compatible replacement.
- State only facts supported by the checked-in source, build files, or README.
- Do not modify kernel, user-space, build, or C reference code.
- Do not modify `docs/migration.md` in this task.

---

### Task 1: Rewrite the architecture reference

**Files:**
- Modify: `docs/architecture.md`
- Verify: `README.md`, `Cargo.toml`, `Makefile`, `kernel/Cargo.toml`, `kernel/src/**`, `user/Cargo.toml`, `user/src/**`, `user-lib/src/**`

**Interfaces:**
- Consumes: the current repository topology, the C reference implementation in `kernel/*.c` and `user/*.c`, and the Rust migration in `kernel/src`, `user/src`, and `user-lib/src`.
- Produces: an accurate Markdown guide linking subsystem responsibilities to the authoritative source paths and build entry points.

- [x] **Step 1: Collect authoritative facts for every section**

Run:

```bash
rg -n '^(pub )?(fn|struct|enum|const|mod) |^#\[|^members|^name|^target' kernel/src kernel/Cargo.toml user/Cargo.toml user/src user-lib/src Cargo.toml
rg -n '^(OBJS|UPROGS|qemu:|fs.img:)|kernel/.*\.c|user/.*\.c' Makefile
```

Expected: paths and symbols needed to map boot, traps, memory, processes, filesystem, drivers, syscalls, user programs, and build paths without relying on old prose.

- [x] **Step 2: Replace stale architecture prose with the migration map**

Write `docs/architecture.md` with these sections:

```markdown
# xv6-riscv Rust Port Architecture

## Scope and status
## Repository layout and source correspondence
## Boot and privilege transitions
## Memory and address spaces
## Processes, scheduling, and synchronization
## File system and descriptors
## Devices and interrupts
## System calls and user space
## Build and image assembly
## Verification and limitations
```

For each subsystem section, name the Rust module path, the C xv6 counterpart, and an explicit status such as `Ported structure`, `Implemented behaviour`, or `Migration limitation / parity not asserted`. Remove the obsolete `xtask`, Linux user-target, generic syscall-dispatcher, unconditional RAII/page-table, integration-test, and performance-parity claims.

- [x] **Step 3: Validate the documentation mechanically and by source lookup**

Run:

```bash
git diff --check -- docs/architecture.md
rg -n 'xtask|all existing usertests|Performance Goals|same asm|syscall\(num' docs/architecture.md
rg -n 'riscv64gc-unknown-linux-gnu' docs/architecture.md
rg -n '`(kernel|user|user-lib|build_rust_users\.sh|run_usertests\.sh|Makefile)[^`]*`' docs/architecture.md
```

Expected: `git diff --check` produces no output; the stale-claim search has no matches; the Linux target appears once in the explicit migration-limitation paragraph; every backticked source reference resolves to a current file or directory.

- [x] **Step 4: Review the resulting diff**

Run:

```bash
git diff -- docs/architecture.md
```

Expected: the diff updates only architecture documentation and contains no claims that Rust feature parity is complete or tested unless directly cited by the repository.

- [x] **Step 5: Commit the completed documentation set**

Run:

```bash
git add docs/architecture.md docs/superpowers/specs/2026-07-24-architecture-documentation-design.md docs/superpowers/plans/2026-07-24-architecture-documentation.md
git commit -m "docs: update Rust port architecture"
```

Expected: one commit containing the architecture guide, its approved design, and this implementation plan; no unrelated `README.md`, `gdb-mcp/`, or pre-existing documentation changes are staged.
