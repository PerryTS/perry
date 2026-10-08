# WASI support: #11375 acceptance assessment

Status: experimental; the complete acceptance scope is **not implemented**.
This assessment uses [#11375](https://github.com/PerryTS/perry/issues/11375)
as its scope and was verified on Windows x64 on 2026-10-08, against main
`6521c0cbf` plus the changes in this branch.

WASI programs run on Windows through a WASI host. We built and ran Perry
components with Wasmtime 48.0.0 and wasi-sdk 34 on Windows. This is the
standalone LLVM `--target wasi` pipeline targeting `wasm32-wasip2`, distinct
from Perry's browser `wasm`/`web` targets and their JavaScript host runtime.
See [Wasmtime's platform support](https://docs.wasmtime.dev/stability-platform-support.html).

## What this branch fixes

- `BufferHeader` was 12 bytes on ILP32, while the byte store ABI requires
  offset 16. Eight-byte alignment preserves the native layout and makes the
  WASI runtime compile without changing the link field's offset.
- The linker now finds `clang.exe` in Windows wasi-sdk installations.
- Runtime ABI source paths are normalized on Windows. Previously the checker
  saw no runtime symbols because its prefix checks expected `/`; emission now
  also refuses to replace the ABI table with an empty table.
- A checksum-pinned PowerShell tool installer and a portable Python runtime
  builder make the standalone pipeline usable on Windows. The builder accepts
  Cargo's fresh cached artifacts and verifies that Cargo reported a real WASI
  archive. Shell entry points delegate to the portable implementations.
- Executable acceptance probes retain compile logs, stdout, stderr and JSON
  results. Compile errors, missing/stale artifacts, timeouts and incorrect exit
  statuses fail the run. The gap suite requires the repo's pinned Node oracle;
  failing oracles fail coverage rather than silently dropping cases.

## Measured acceptance

The existing smoke suite passes **6/6**: classes, closures, collections,
exit codes, iterators and values. The additional acceptance suite passes
**6/11**, with the results below. These are focused probes, not exhaustive
proofs of feature compatibility.

| Probe | Windows result | What it establishes or exposes |
| --- | --- | --- |
| Byte arrays / DataView | Pass | Allocation, indexing and slice with the corrected header |
| UTC dates | Pass | Fixed UTC date formatting and timezone offset |
| Files | Pass | Read/write/delete with an explicit directory preopen |
| GC | Pass | Retained object graph across collections with forced evacuation and verification enabled |
| Tagged values | Pass | Minimal #11412 prototype case plus defineProperty / Reflect.get; broader cases still fail below |
| Timers | Pass | Promise microtask followed by a timer; the existing WASI event-pump path handles this case |
| Exceptions | Fail at runtime | Nested throw/finally/catch terminates instead of unwinding |
| DNS | Fail at runtime | `lookup('localhost')` returns ENOTFOUND even with inherited networking |
| TCP | Fail at link | Missing `js_ext_net_*` / `js_net_*` symbols |
| Threads | Fail at runtime | Sequential parallelMap/filter work; spawn rejects as unsupported |
| Child processes | Fail at runtime | Missing-command error says UNKNOWN and cannot be caught; requires an explicit unsupported-operation error |

Ten representative existing gap programs were also compared with Node
26.5.1: **5/10 pass**. Array methods, string methods, closures, extended Map/Set
and error extensions pass. Advanced classes and object methods terminate at
expected catches. Advanced JSON and regexp encounter `Reflect.defineProperty
called on non-object`. Advanced async traps with an indirect call type mismatch
in `closure::dispatch::calln::dispatch_call_slice`. The latter needs callback ABI
work even after exception transport exists.

The passing six acceptance cases form `--suite core`. CI runs those cases and
the five passing Node comparisons in the existing WASI codegen job. The full
acceptance and full ten-case gap runs deliberately remain failing diagnostics.
The existing CI job is Ubuntu-only and conditional on compatibility routing
and the PR's `run-extended-tests` label; these changes do not make WASI a
required PR check or establish Linux/macOS parity with this Windows run.

## Remaining implementation work, in dependency order

1. **Exception transport and GC restoration (#11378).** The WASI implementation
   of `perry_sjlj_try` currently just calls its body; `js_throw` exits rather
   than reaching a catch. The runtime build excludes the native C trampoline
   on WASI. Select and implement a matching wasm exception strategy across
   LLVM codegen, libc/compiler support and runtime boundaries. If following
   the issue's SjLj design, provide the WASI trampoline and matching compiler
   flags/libraries; the native invoke/landingpad/personality path cannot simply
   be assumed to work. Save and restore GC shadow-stack/savepoint state across
   unwinds. Require nested catch/finally, rethrow, runtime-originated errors,
   callback errors and rejected async errors to pass with forced GC. A jump
   target must remain live; a Rust wrapper that returns before longjmp is not
   a safe substitute.

2. **Complete the ILP32 ABI audit (#11412 and #11378).** The generated runtime
   ABI adapter and closure receiver adaptation cover some calls. The minimal
   prototype reproduction now passes, but the JSON/regexp failures show that
   it is premature to mark the boxed-value problem fixed. Inventory parameters
   which semantically carry 64-bit tagged values through pointer-shaped APIs;
   preserve the tags instead of narrowing to wasm's 32-bit pointer width.
   Regenerate and check `runtime_abi.tsv` after signature changes. Separately
   audit runtime-created callback signatures, arities and indirect table calls
   using the advanced async trap. Test callbacks from both generated code and
   runtime code, including functions called with fewer or extra arguments.

3. **WASIp2 networking and polling (#11377).** `run_pipeline.rs` currently
   links the runtime archive alone for WASI; the TCP fixture needs extension
   symbols as well as a WASI network implementation. Define a WASI feature
   profile for the extension archives, integrate socket readiness with the
   event pump, and use host-supported address resolution. Merely compiling
   socket2 or linking an archive does not establish usable networking. Test
   loopback TCP accept/connect/read/write/close, DNS results and errors, and
   promises/timers progressing while I/O is pending. Make preopens and network
   capabilities explicit, with catchable errors when denied. Existing simple
   timers pass; test cancellation and multiple pending timers during this work.
   UDP's `mod-dgram` remains excluded from the runtime build and must be
   addressed if the final socket scope includes UDP.

4. **Sequential thread fallback (#11377).** Keep the existing single-thread
   map/filter behavior and implement spawn as a deferred task in the current
   agent, with the closure and result promise rooted until completion. Do not
   inline the native worker lifecycle: it claims/retires an agent and can
   invalidate the main heap. Test captured references, returned values,
   rejection, observable ordering and GC during pending work. This fulfills
   the issue's fallback scope without requiring shared-memory wasm threads.

5. **Portable platform contracts (#11377).** WASI has no general host process
   spawning API. Return a clear, catchable unsupported-operation error for
   child_process instead of UNKNOWN or a trap. Keep the documented UTC date
   fallback, and document host filesystem/network rights and unsupported
   platform-specific APIs. Extend the probes to environment, arguments and
   deliberate permission denial before claiming portable CLI coverage.

6. **Linking and acceptance gates (#11379 / #11380).** Keep the current build,
   ABI, smoke, core and oracle checks; promote repaired cases into the gate
   rather than suppressing their failures. Expand the Node parity selection
   after the callback/exception fixes, run the native regression checks, add
   Windows host CI, and establish Linux/macOS host results. Consolidate the
   WASI tool pins with the repository's tool-version policy. Only mark #11375
   complete when every agreed capability has end-to-end coverage and all
   acceptance probes pass.

These are separable implementation PRs, with exceptions and ABI work unlocking
reliable diagnostics for the later platform work. The current branch supplies
build repairs and reproducible evidence, not the remaining runtime designs.

## Reproducing on Windows

Use the repository's pinned Rust toolchain and LLVM 22 setup first. From the
repository root in PowerShell (Python 3.10+ required):

```powershell
. ./scripts/wasi_toolchain.ps1
rustup target add wasm32-wasip2
cargo build --locked -p perry --no-default-features --features compile-cli,target-wasi
python scripts/wasi_build_runtime.py
python scripts/wasi_run.py target/debug/perry.exe --suite smoke --output .cache/wasi-smoke
python scripts/wasi_run.py target/debug/perry.exe --suite core --output .cache/wasi-core
python scripts/wasi_run.py target/debug/perry.exe --suite acceptance --output .cache/wasi-acceptance
python scripts/wasi_run.py target/debug/perry.exe --suite gap --node <path-to-pinned-node> --output .cache/wasi-gap
```

The installer pins SDK 34 and Wasmtime 48, sets the SDK/compiler/runtime
environment variables and supports Windows x64 and ARM64. Only x64 was tested.
The runtime builder enables the runtime's current default feature set except
`alloc-mimalloc` (64-bit-only) and `mod-dgram` (unsupported UDP turnloop path),
matching `wasi_check.sh`.

On Linux/macOS, configure the tools using
`eval "$(./scripts/wasi_toolchain.sh)"`, then use the same Python commands with
the platform's compiler path. `wasi_build_runtime.sh` and `wasi_smoke.sh` remain
compatible shell entry points. `--filter` accepts repeatable name substrings;
each fixture runs in a separate working directory. Without `--output`, logs
are temporary. With it, `report.json` and per-case logs are retained.

Harness regression checks:

```powershell
python scripts/test_wasi_run.py
python scripts/runtime_abi_check.py --self-test
python scripts/runtime_abi_check.py --check-wasm-abi
cargo test --locked -p perry --bin perry --no-default-features --features compile-cli,target-wasi links_a_wasip2_component -- --nocapture
```

The Windows run built the release WASI runtime with the supported feature
profile, checked the runtime with no default features, built the compiler,
passed the linker test, passed six harness
regression tests, and confirmed the ABI table is current. Raw measurements
are retained locally under `.cache/wasi-*`; generated artifacts are ignored.
