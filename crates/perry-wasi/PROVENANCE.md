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
| `src/pipe.rs` | Memory and closed WASI stream implementations in `wasmtime-wasi/src/p2/pipe.rs` (without Tokio adapters) |

Native readiness now returns a fallible result: timer/driver failures trap
through WASI poll/block/ready instead of reporting successful readiness. The
poll list still coalesces duplicate resources while reporting every ready input
index. Resource parent/child ownership and stream error conversion are retained.
Synchronous bindings wait on native futures with `futures-lite`; I/O and timer
work is owned and driven by Turnloop, with no ambient Tokio runtime.

The current port includes WASI 0.2.12 I/O, clocks and random interfaces. It is
not yet selected by `perry-component-host`. CLI, capability filesystem, sockets,
HTTP and native WASI 0.3 remain to be ported and qualified. No missing interface
is implemented as a success stub. The workspace Tokio gate remains unchanged
and will not pass until the old provider dependency is removed.
