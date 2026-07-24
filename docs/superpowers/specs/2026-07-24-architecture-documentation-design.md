# Architecture Documentation Update Design

## Outcome

Rewrite `docs/architecture.md` in English as an evidence-based overview of the
Rust port of xv6-riscv.  The document will describe the Rust implementation as
the primary subject while showing its correspondence to the canonical C xv6
sources kept in this repository.

## Scope

- State explicitly that the Rust kernel is an in-progress migration, not a
  complete replacement for C xv6.
- Organize the document around the actual subsystem boundaries and execution
  paths: boot, virtual memory, traps, processes and scheduling, filesystem,
  drivers, syscalls, user space, and builds.
- For every subsystem, identify the relevant Rust paths and the corresponding
  C xv6 paths, then distinguish implemented behaviour from known limitations
  or unverified parity.
- Document the two build paths separately: the root `Makefile` builds the C
  reference system; Rust build scripts build and package the port.
- Replace stale module trees, APIs, targets, dependency claims, testing claims,
  and unsupported performance guarantees with statements traceable to source.

## Non-goals

- Do not change the kernel, user programs, build scripts, or C reference
  implementation.
- Do not claim feature completeness or test compatibility without evidence.
- Do not attempt a line-by-line migration guide; `docs/migration.md` remains
  outside this change.

## Verification

- Check every listed module path and source mapping exists.
- Compare build instructions with `README.md`, the root `Makefile`, and Cargo
  manifests.
- Review the final document for stale `xtask`, Linux-target, generic syscall
  dispatcher, and performance/test-completeness claims.
