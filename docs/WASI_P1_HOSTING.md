# Native WASI Preview 1 hosting

Perry's optional `perry-wasm-host` archive executes core WebAssembly modules
through wasmi 1.1 and implements Preview 1 syscalls through the synchronous
`wasi-common` provider. This is **WASI 0.1 input support** for native Perry.
`--target wasi` continues to emit the separate WASI 0.2 command profile; it
does not emit Preview 1 modules.

```ts
import { WASI } from "node:wasi";
import { readFileSync } from "node:fs";

const wasi = new WASI({
  version: "preview1",
  args: ["guest", "argument"],
  env: { MODE: "example" },
  preopens: { "/sandbox": "./guest-data" },
  returnOnExit: true,
});
const module = new WebAssembly.Module(readFileSync("guest.wasm"));
const instance = new WebAssembly.Instance(module, wasi.getImportObject());
console.log("Guest exit status:", wasi.start(instance));
```

Compile this native host with `perry compile host.ts --enable-wasm-runtime`.
Ordinary `WebAssembly` references also select the optional host automatically.
The matching `libperry_wasm_host.a` must be installed alongside the runtime.
Programs using neither WebAssembly nor WASI execution do not link this archive
or its filesystem provider. Creating only the `WASI` API shape does not require
an engine; actual syscall execution does.

## Supported behavior

- Command lifecycle through `start`; reactor lifecycle through `initialize`,
  with subsequent exported functions retaining the context through their
  imports. Argument, environment and preopen options are snapshotted.
- All 46 Preview 1 imports dispatch through the maintained provider: descriptor
  I/O, files/directories, metadata, seeking, clocks, entropy and synchronous
  `poll_oneoff`. Rights, descriptor kinds and host OS support determine the
  errno for a particular operation.
- Each WASI object owns its own descriptor table and preopens. No directory or
  environment is inherited implicitly. Stdin, stdout and stderr are inherited
  unless explicitly redirected; clocks and entropy are provided by the host.
- Configured Perry `fs` descriptors are translated to duplicated OS handles.
  Guest close and context collection release those duplicates. They do not
  close the embedding application's original descriptors. This differs from
  Node's handling of an explicitly supplied stdio descriptor.
- `proc_exit` unwinds the guest before its next instruction. With
  `returnOnExit: true`, `start` returns the unsigned 32-bit status; with false,
  the embedding process exits. User-provided JavaScript import replacements
  remain ordinary functions, including a replacement named `proc_exit`.
- Syscalls borrow the current linear-memory span. The import boundary refreshes
  it after `memory.grow`; no persistent slice points at the old allocation.
- Missing function imports fail linking. Invalid syscall signatures, invalid
  lifecycle state and invalid guest pointers fail execution rather than
  supplying fabricated successful results.

The syscall context is an owned native GC payload reachable from the import
object. It contains no Perry heap pointers and is not registered in a global
capability map. The existing per-agent shared wasmi store and JavaScript
callback re-entry model are preserved. Dropping a context closes its remaining
preopens and files independently of the shared store's lifetime.

## Limits

- This is core Preview 1 hosting, not Component Model hosting or WIT bindings.
  P2 reusable components and P3 async interfaces remain separate roadmap items.
- `version: "unstable"` retains the existing API shape, but Preview 0 execution
  is explicitly unsupported. Its different data layouts must not silently use
  Preview 1 bindings.
- No socket capabilities are exposed by the constructor. The socket imports
  cannot create connections or listeners; unsupported signal operations such
  as `proc_raise` follow the provider's unsupported/trap behavior.
- The synchronous provider can block the native thread during I/O and polling.
  Async WASI/component scheduling and guest execution quotas are not provided.
- Node-specific error messages and codes are not a complete compatibility
  promise. Guest pointer failures can surface as WebAssembly runtime errors.
- Execution is qualified on Linux x86_64 and macOS ARM64. Windows qualification
  remains pending; provider availability alone does not establish execution.

## Validation

Host crate tests cover arguments/environment, clocks/entropy, memory bounds,
filesystem operations, preopen confinement (including symlink escape), separate
context descriptor tables, polling, stdio ownership, descriptor release,
unsigned exit statuses, missing imports and existing callback re-entry.

The Node suite includes `host-capabilities.ts`, `context-lifetime.ts`,
`exit-stops-guest.ts`, `exit-reentry.ts`, and a wasi-sdk 34 libc command testing arguments,
environment, create/read/seek/close/unlink and scatter/gather output. WAT and C
sources accompany the checked-in Wasm fixtures; the C source records its build
command. Compare against the repository's `.node-version` oracle.

Linux x86_64 qualification passed 26 host crate tests, the direct/aliased WASI
constructor lowering regression, and all 56 WASI Node cases against Node
26.5.1 (55 stdout comparisons plus the expected process-exit status 7 case).
The retained-export case also passed with explicit garbage collections.
The changed runtime passes `cargo check --target wasm32-wasip2` with wasi-sdk
34, retaining source compatibility with the standalone P2 target.

```sh
RUST_TEST_THREADS=1 cargo test -p perry-wasm-host
NODE_BIN=/path/to/pinned/node python3 scripts/node_suite_run.py \
  /path/to/perry "$PWD" wasi
```

See [WASI_ROADMAP.md](WASI_ROADMAP.md) for subsequent component work and
[WASI_SUPPORT.md](WASI_SUPPORT.md) for the standalone P2 output profile.

The [native platform receipt](validation/wasi-native-host-platforms-20261021.json)
records source `17318d3ad30db9d43fc09193e3efb1a3f1729381` passing all 56
P1 oracle comparisons plus the moving-GC context-lifetime case on Linux x86_64
and macOS ARM64. It also records 26 Linux / 25 macOS P1-host unit tests and
retained output identities. Windows is not included in those passing results.
