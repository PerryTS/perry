Install the pinned WASI C compiler and sysroot before the preliminary runtime
CI check. The Rust WASI target alone cannot compile the runtime's `hot_text.c`;
the job previously invoked system clang against host libc headers and failed.

Make `wasi_check.sh` report a missing C toolchain directly. Check every default
runtime feature except the documented 64-bit allocator exclusion, matching the
linkable runtime build instead of silently omitting `bun-cli-utils`.
