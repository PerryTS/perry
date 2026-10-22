# Provider provenance and migration scope

This crate adapts WASI contracts and native helpers from Bytecode Alliance
Wasmtime **48.0.5**, distributed under **Apache-2.0 WITH LLVM-exception**. The
full license is in `LICENSE`. The source packages are `wasmtime-wasi` and
`wasmtime-wasi-io`, both pinned to 48.0.5 in Perry's dependency lockfile.

Copied/adapted sources:

| Perry source | Upstream source |
| --- | --- |
| `wit/deps/*.wit` | `wasmtime-wasi/src/p2/wit/deps/*.wit` |
| `src/io/{bindings,impls,poll}.rs` | `wasmtime-wasi-io/src/{bindings,impls,poll}.rs` |
| `src/io/sync_poll.rs` | `wasmtime-wasi/src/p2/poll.rs` |
| `src/io/sync_streams.rs` | `wasmtime-wasi/src/p2/host/io.rs` |
| `src/random.rs` | `wasmtime-wasi/src/random.rs` |
| `src/random/host.rs` | `wasmtime-wasi/src/p2/host/random.rs` |
| `src/clocks/system.rs` | Native clock helpers in `wasmtime-wasi/src/clocks.rs` |
| `src/filesystem.rs`, `src/filesystem/` | `wasmtime-wasi/src/filesystem.rs` and its capability primitives, platform helpers and tests |
| `src/p2/filesystem.rs`, `src/p2/host/filesystem*` | `wasmtime-wasi` files with the same paths |
| `src/p2/stdio.rs`, `src/p2/host/{env,exit}.rs` | `wasmtime-wasi` files with the same paths |
| `src/error.rs`, `src/clocks/datetime.rs` | `wasmtime-wasi/src/error.rs` and timestamp conversions from `src/clocks.rs` |
| `src/pipe.rs` | Memory and closed WASI stream implementations in `wasmtime-wasi/src/p2/pipe.rs` (without Tokio adapters) |

Native readiness now returns a fallible result: timer/driver failures trap
through WASI poll/block/ready instead of reporting successful readiness. The
poll list still coalesces duplicate resources while reporting every ready input
index. Resource parent/child ownership and stream error conversion are retained.
Synchronous bindings wait on native futures with `futures-lite`; I/O and timer
work is owned and driven by Turnloop, with no ambient Tokio runtime.

The current port includes WASI 0.2.12 I/O, clocks, random, CLI and capability filesystem interfaces. It is
not yet selected by `perry-component-host`. Sockets,
HTTP and native WASI 0.3 remain to be ported and qualified. No missing interface
is implemented as a success stub. The workspace Tokio gate remains unchanged
and will not pass until the old provider dependency is removed.

Filesystem operations use explicit weak submission endpoints and Turnloop's
bounded blocking pool. Only owned Rust/native handles and buffers cross the
worker boundary. Native file errors retain WASI error codes; driver errors trap.
Dropping a stream cancels pending delivery and queued work. A syscall already
running can finish, retaining its native inputs; cancellation cannot undo its
side effects. This limitation also applies to shutdown. No guest heap pointer,
callback, canonical ABI memory or Wasmtime Store enters the worker pool.

The capability primitives retain upstream path resolution and permission checks.
Mechanical lint/format changes adapt them to Perry's toolchain. The ambient Unix
permission tests require an unprivileged test process; running as root defeats
their permission-denial fixture. No test is ignored to accommodate root.

CLI arguments/environment/cwd are explicit context capabilities. Defaults use a
closed input and discarded output, with no inherited environment or terminal.
Inherited stdio uses Turnloop native duplicate/classify/read/write/close operations;
it does not use blocking worker reads. Guest stdin handles share one bounded
read and cursor. The last live input view cancels a pending read, retaining any
already-received bytes for later views. Each output view has one bounded write,
requires readiness before another write, and explicitly flushes native writes.
Native I/O errors retain their OS code, then close the affected stream; driver
failure traps. Process standard handles are borrowed through native duplicates.

The copied sockets WIT file currently resolves CLI world references only. Socket
implementations are still absent, and the provider world does not import them.
