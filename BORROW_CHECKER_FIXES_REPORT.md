# Borrow Checker Fixes Report - Task 7 (File System)

## Summary
Fixed 7 borrow checker errors in the kernel's file system implementation (Task 7). The kernel now builds successfully.

## Files Modified

### 1. kernel/src/fs/buf.rs
**Errors Fixed: E0515, E0502, E0505 (3 errors)**

- **BufRef::lock()** (E0515): Fixed "cannot return value referencing local variable `cache`" by using unsafe pointer to access the static BUF_CACHE buffer directly, since BUF_CACHE is a global static and buffers live for the entire program lifetime.

- **bget()** (E0502, E0505): Fixed "cannot borrow `cache` as mutable because it is also borrowed as immutable" and "cannot move out of `cache` because it is borrowed" by restructuring the LRU buffer reuse logic to drop the guard before calling `cache.move_to_head()`.

- **bread()** (E0505): Fixed "cannot move out of `cache` because it is borrowed" by separating the validity check (with cache lock) from the actual I/O operation (with a fresh cache lock acquisition).

### 2. kernel/src/fs/pipe.rs
**Error Fixed: E0502 (1 error)**

- **Pipe::clone()** (E0502): Fixed "cannot borrow `inner` as immutable because it is also borrowed as mutable" by extracting all needed fields from the lock guard before dropping it, then using the extracted values to create the new Pipe.

- **Pipe::write()**: Fixed overlapping borrow issue by computing the needed size before the mutable borrow.

### 3. kernel/src/proc/syscall.rs
**Errors Fixed: E0505 (2 errors)**

- **sys_read()** and **sys_write()** (E0505): Fixed "cannot move out of `inner` because it is borrowed" by:
  1. Changing `let inner = p.lock()` to `let mut inner = p.lock()`
  2. Using `filedup()` instead of `.clone()` on `Option<File>` (since File doesn't implement Clone due to containing a SpinLock)
  3. Passing `&f` to `fileread()` and `filewrite()` which expect `&File`

### 4. kernel/src/fs/inode.rs
**Error Fixed: E0596 (1 error)**

- **balloc()** (E0596): Fixed "cannot borrow `buf` as mutable, as it is not declared as mutable" by changing `let buf = bp.lock()` to `let mut buf = bp.lock()`.

### 5. kernel/src/fs/file.rs
- Removed `#[derive(Clone)]` from `File` struct since it contains a `SpinLock` which cannot be cloned.

## Verification
```bash
$ cargo build -p xv6-kernel
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.02s
```

All 7 borrow checker errors have been resolved. The kernel compiles successfully.