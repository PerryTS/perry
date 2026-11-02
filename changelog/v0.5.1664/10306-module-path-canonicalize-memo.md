Module path canonicalization is memoized per directory. Registering a module
canonicalizes its absolute path, and `std::fs::canonicalize` is a full
realpath: one `readlink` for every path component, every time. Sibling modules
share every ancestor, so the same prefixes were re-walked once per module.

Running `opencode --version` issued 86,745 `readlink` calls over only 9,467
distinct paths — 98.7% of every syscall the process made and 0.32s of system
time. The tree root alone was resolved 7,501 times and `node_modules/.bun`
5,580 times.

Directories are now resolved once and reused, so each additional module in a
directory costs one `readlink` for its own basename instead of one per path
component: a 400-module fixture drops from 4,010 `readlink` calls to 405. A
path containing `.` or `..` components, or a basename that really is a symlink,
still goes through `std::fs::canonicalize`, so resolution semantics are
unchanged. This is wall-clock, not instruction count — it moves `instructions:u`
by 0.04%.
