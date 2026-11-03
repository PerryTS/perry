`perry-runtime-static` builds for `wasm32-wasip1` (part of #13259). The runtime
archive imports only `wasi_snapshot_preview1`; there is no `--target wasip1`
yet. `scripts/wasi_build_runtime.py` (and `.sh`) and `scripts/wasi_check.sh`
take the target as an optional argument and default to `wasm32-wasip2`, whose
build is unchanged, and `scripts/wasi_toolchain.sh`/`.ps1` also configure the C
compiler for `wasm32-wasip1`. On preview 1:

- `perry_wasi_net.c` has a preview 1 arm: every socket operation reports
  `ENOTSUP`, and `listen` reports `EADDRNOTAVAIL`, because preview 1 cannot
  create sockets or resolve names.
- `socket2` is not a dependency on WASI.
- `Atomics.waitAsync` throws a `TypeError`; `process.stdin`, animated TUI
  spinners and TUI raw mode start no background thread.

`scripts/wasi_imports_check.py` reads the import section of a WASI module or
archive; the required `wasi-execution` job builds the preview 1 runtime and the
preview 1 image-classifier selftest archive and runs it on both. Running the
selftest needs a linked program, which comes with the wasip1 compiler target.
