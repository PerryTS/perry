`perry-runtime` builds for the standalone WASI target, `wasm32-wasip2` (phase
2a of #11375, #11377). Every platform-specific path has a WASI branch, and on
every other target the code is unchanged. On WASI:

- `child_process` fails through its ordinary spawn-error path (WASI has no
  process spawning).
- Local time is UTC.
- `os.hostname()` is `"localhost"`.
- Native plugins fail to load with a clear message.
- `fs.symlink` uses WASI's own call.
- `parallelMap`/`parallelFilter` run sequentially.
- `perry/thread` `spawn()` rejects with "not supported on WASI yet".

Timers ride the existing wasm32 park, which on single-threaded WASI is a timed
sleep. The exception trampoline `perry_sjlj.c` is not built for WASI until the
wasm EH link lands (#11378/#11379). `scripts/wasi_check.sh` checks the core
runtime and every default feature except the named WASI exclusions:

- `bun-cli-utils`: zstd needs wasi-sdk;
- `mod-dgram`: turnloop;
- `alloc-mimalloc`: 64-bit only.

The new `wasi-check` workflow runs it daily and on labelled PRs; it is
intentionally not a required context yet.
