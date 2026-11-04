WASI (#13953): `--target wasm32-wasip2` and `--target wasip2` now build the
same WASI 0.2 component as `--target wasi`, and `--target wasm32-wasip1`
builds the preview 1 module. These names used to fall through to a host link
that failed with `unable to disambiguate: -dead_strip`. `wasip3` and
`wasm32-wasip3` now fail with an error naming the WASI targets that exist.
A WASI 0.2 program that iterates `process.stdin` (`for await`) no longer panics with `failed to
spawn thread`: the event loop polls fd 0 as on preview 1 instead of starting a
reader thread. The TUI input reader, spinner ticker and `Atomics.waitAsync`
take the same no-thread path on WASI 0.2 that they already took on preview 1.
`docs/WASI_SUPPORT.md` gains a target-name table and notes that
`navigator.userAgent` reports `Node.js/22` as a compatibility value.
