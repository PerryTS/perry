# WASI hosting and reusable components

Implementation roadmap, updated 2026-10-26. P2 commands and P1 hosting are
implemented. Typed components work in both directions for the synchronous
Linux profile. Native P3 hosting includes concurrent calls, typed future/stream
readers and producers, and supported concurrent HTTP handlers. Async
Perry-authored guest adapters and full platform/package qualification remain open.
The audited baseline is
`20fde48d3fa03d0a77699b7374f55e38cb143b59`.
See [WASI_SUPPORT.md](WASI_SUPPORT.md) for the implemented standalone profile.

| Format and direction | Implemented profile | Remaining gap |
| --- | --- | --- |
| WASI 0.1 / Preview 1 input | Native `node:wasi` hosting; see [P1 scope](WASI_P1_HOSTING.md) | No P1 compiler output target; intentionally deferred |
| WASI 0.2 / Preview 2 output | Standalone commands; staged packages qualified on Linux x86_64, macOS ARM64 and Windows x64 | Requalify changed artifacts before shipping; publication is separate |
| Typed synchronous components | Native Perry hosts third-party components; Perry libraries import/export WIT services and compose with Rust | Current combined execution qualification is Linux; broader platform/package coverage remains open |
| WASI 0.3.0 input | Turnloop providers, async calls, native TypeScript future/stream readers and producers, command hosting and supported concurrent HTTP layouts | Nested/imported HTTP implementations use serialized fallback; broader platform qualification and final packages remain open |
| Async Perry-authored components | Rooted frames and nonblocking Promise observation foundations | Generator still rejects async guest functions and future/stream values; task scheduling, suspension, cancellation and cross-language acceptance remain unfinished |

The 0.3.0 hosting profile does not establish a 0.3 compiler output target or
complete bidirectional async support. Detailed current evidence is linked from
[the component record](WASI_COMPONENTS.md); older receipts describe their own
source snapshots and should not be treated as qualification of every later change.

## Decision: support P1 inputs, defer P1 output

Support WASI 0.1 / Preview 1 as an interoperability format for running
third-party core modules inside native Perry. Do not introduce a P1 compiler
output target until an actual deployment requires it. P1 still has broad
runtime adoption, but it does not supply the typed component interfaces
needed for reusable plugins. New authoring should use the Component Model,
with a tested P2 profile and P3 native async as a subsequent milestone.

P1 output would need its own runtime build, sysroot/link recipe, supported
API profile, packaging and CI. Reusing a wasm32 layout is not sufficient:
Perry's P2 networking and exception-handling requirements must also work on
the proposed P1 host. If a P1-only deployment becomes necessary, qualify that
specific host and API subset before expanding the public target list.

This separates three deliverables:

1. Perry TypeScript programs running as standalone WASI commands.
2. Native Perry applications hosting third-party Wasm modules/components.
3. Perry TypeScript libraries exported through WIT, callable by other
   languages, and importing WIT services in return.

Neither a Wasm filename nor a command component proves all three work.

## 1. Establish a dependable P2 command baseline

The audit reproduced a panic while parsing the checked-in runtime ABI table.
Four plain `i32` tokens were invalid, and regeneration also found changed
signatures: in particular, the `js_try_push` stack-mark argument had gone
missing. Refresh the full generated table and test exception lowering against
the real table; changing only the invalid tokens leaves ABI drift behind.

Fresh compilation after that repair exposed three further runtime blockers:

- The WASI net module reuses `turnloop_net/abi.rs`, whose newer transport
  entry points refer to native transport types/functions unavailable in the
  WASI module. Repair the target-specific transport contract and unsupported
  paths before re-running TCP/denial probes.
- `PerryLayoutDesc`, method descriptors, atoms and `BandDir` assert 64-bit
  sizes/offsets on wasm32. Make the runtime and generated code agree on the
  32-bit data layout, including embedded pointer fields and accesses. Merely
  disabling assertions would remove the guard without fixing the ABI.
- Pointer-bearing runtime records are emitted into custom Wasm sections,
  which Rust rejects. The existing Wasm `layout_bounds()` and `atom_bounds()`
  also return empty ranges. Provide linear-memory records and a working
  initialization/registration path; removing section attributes alone would
  leave static layouts unregistered.

Status: these build blockers are repaired. Execution also found and fixed
missing macro-export signatures, internal array FFI pointer mismatches and a
WASI link path that omitted the runtime feature installer. Linux validation:
6/6 smoke, 20/20 acceptance and 10/10 pinned-Node comparisons pass; see
[the support document](WASI_SUPPORT.md) for the exact scope and tools.
Fresh compiler/runtime builds pass all 36 execution checks on Linux,
macOS ARM64 and Windows x64, with pinned-Node comparisons. PR #12954
repaired the Windows object-buffer defect; its fresh Windows CI run passes.
PR #12955 promotes the qualified Linux command profile into the required
execution gate. Matching compiler/runtime packaging is implemented, and the
final staged archives passed all 36 checks on Linux x86_64 GNU, macOS ARM64
and Windows x64. See the [packaging receipt](validation/wasi-release-artifacts-20261021.json).
That qualification used build-only mode; publication remains a separate
maintainer release action.

Acceptance:

- ABI generation and Rust parsing agree, including exception stack marks.
- Build the compiler and wasm32-wasip2 runtime from the same source revision.
- Pass the complete smoke, acceptance and pinned-Node comparison suites;
  record host, versions, compile/runtime failures and required capabilities.
- Validate emitted components and inspect their WIT imports/exports. Include
  denied filesystem/network access, forced GC and callback exceptions.
- Route relevant compiler/runtime changes to a required P2 execution check
  once that check has a demonstrated green baseline. The existing ABI lint
  is already present; the expensive WASI execution jobs are conditional.
- Publish matching compiler/runtime artifacts and clear host prerequisites.
  The Unix tool installer now pins x86_64/ARM64 Linux and macOS assets;
  the new platform paths still need full Perry qualification.

## 2. Make P1 hosting operational

Keep `node:wasi` compatibility separate from the standalone output target.
The implementation uses `wasi-common` directly across the optional archive
boundary, keeping the existing wasmi 1.1 store unchanged. Each WASI import
object owns a collecting native context. See [WASI_P1_HOSTING.md](WASI_P1_HOSTING.md)
for supported behavior, qualification and explicit limits.

The integration must preserve per-WASI-instance arguments, environment,
preopens, descriptors and lifecycle. Perry currently shares a wasmi store
between objects in one JavaScript agent and permits callback re-entry; a
single global WASI context would leak capabilities across instances. Bind
imports to the right context, access guest memory through the active caller,
and retain the existing re-entry discipline. Default to no ambient directory
or network access; explicitly supplied capabilities determine access.

Acceptance is execution behavior, not the number of import names:

- Real `fd_write` hello-world, redirected stdio, scatter/gather reads/writes,
  arguments and environment from a third-party Rust or C P1 fixture.
- File create/read/seek/close through a preopen, denied traversal and symlink
  escape, no access through another instance's descriptors.
- Host entropy, supported clock IDs and monotonic behavior, polling and
  invalid-pointer/descriptor failures. Invalid requests must not corrupt the
  host or report false success.
- `proc_exit` stops guest execution immediately, including a sentinel store
  after the call that must never execute. `returnOnExit` controls the enclosing
  JS API's behavior; it must not make the guest import return normally.
- Missing/mistyped imports fail linking; unsupported operations return the
  specified error rather than pretending to work. Define socket and legacy
  `wasi_unstable` scope explicitly; neither follows from Preview 1 support.
- Run relevant upstream WASI tests plus Perry lifecycle, memory growth,
  multiple-instance, re-entry and cleanup tests.

This is a substantial host integration, not an extra compiler-target flag.

## 3. Add typed P2 components in both directions

The initial synchronous slice is implemented and qualified on Linux: native
Perry hosting a Rust component, a Rust host calling a Perry component,
direct Rust/Perry composition, structured-value round trips, native resource
disposal, host-defined resources and Perry-authored guest resources under
moving GC, plus opt-in WASI
providers. See [the component record](WASI_COMPONENTS.md) for the separately
qualified host-defined resources, native/guest deferred cleanup, command
worlds and TCP/UDP/DNS interoperability. Their combined integration is
qualified on Linux. HTTP client and incoming-handler resources and transports
now have separate Linux engine/canonical-component qualification, including real
HTTP payloads, TLS verification and streamed server exchanges through the Rust
embedding API. The TypeScript server API and Perry-authored incoming/outgoing
HTTP components now pass Linux development and production qualification with
independent host/guest moving GC. Blocking-I/O cancellation,
broader platform qualification and engine shipping costs remain open. The Tokio-free Turnloop provider
replacement is now selected and qualified on Linux in both component directions;
its [earlier receipt](validation/wasi-turnloop-selected-20261023.json) records
validation-only upstream repairs. The exact qualified sources now live in the
repository through [pinned Cargo patches](../third_party/turnloop/VENDORED.md),
so builds no longer need those external overrides. Released dependency adoption
remains a future replacement for these local source repairs.
This milestone is not complete.

Introduce an optional component-capable host boundary, for example a separate
`perry-component-host` crate backed by Wasmtime. Keep it off the default native
link path. The existing wasmi-based `WebAssembly.*` API has shared-memory,
table and re-entry contracts; replacing that engine is a separate decision.
Measure the cost of two optional engines before considering consolidation.

Make WIT the interface source of truth. Generate TypeScript declarations and
host/guest adapters from the same world, with interface versions checked at
load/composition time. Do not expose Perry heap pointers or the native
`perry/plugin` shared-arena ABI across the component boundary.

Start with a small synchronous library world that imports a logging service
and exports a transformation accepting a record containing a string and a
byte list, returning a typed `result`. This exercises calls in both
directions, allocation, ownership, errors and structured values. Keep
initialization separate from command `_start`: reusable library components
need repeated calls and explicit lifetime management.

Then cover options, variants, lists, numeric limits and resources. Define
`s64/u64` to BigInt mappings, string encoding, `own`/`borrow`, post-return
cleanup and disposal. Typed WIT errors should retain their declared shape;
guest traps and host failures are a different outcome. Unsupported WIT types
must be diagnosed during binding generation, not silently mapped to `any`.

Acceptance:

- Native Perry calls a Rust-authored component; a Wasmtime/Rust host calls a
  Perry-authored component using the same WIT contract.
- A Perry-authored component imports and composes with another language's
  component without a JavaScript bridge.
- Repeated calls under forced Perry GC preserve values and free transferred
  buffers/resources. Test traps, failed initialization, unload and attempted
  use after disposal, including exactly-once resource destruction.
- Capability grants, memory limits and interruptible execution work per
  component; missing interfaces and version/type mismatches fail clearly.
- Command worlds, custom library worlds and `wasi:http` are separately
  qualified. Existing `node:http` behavior does not establish `wasi:http`.

P1 adapters can make some core modules run in a component host, but they do
not infer typed application exports from an arbitrary P1 binary. Such
exports still need a specified WIT contract and matching bindings.

## 4. Add P3 native async

The optional provider feature implements 0.3.0 clocks/random, CLI, filesystem,
sockets and HTTP through Turnloop. Canonical provider tests cover suspended
calls, streams, result futures, large transfers and Store retirement. The
optional Perry component engine now links these providers and exposes native
async functions through Promise-based TypeScript host bindings. Standalone
component worlds support concurrent calls, and locally observable P3 HTTP
handlers now complete requests independently on one Store. P2 incoming handlers
and nested/imported P3 implementations retain serialized handling. One-shot 0.3.0 command hosting
and TypeScript/Rust async HTTP handler entry points have separate Linux
qualification, including task exit after a result is published. The Rust
persistent driver supports concurrent calls, post-return work and generic WIT
future/stream reads, transfers and native producer factories. Bounded writes
preserve backpressure and unconsumed ownership through cancellation, retry and
peer closure. Traced TypeScript readers, producer factories/writers and
per-invocation runtime frames are implemented. Async guest adapters, broader
handler layout support and complete host/guest composition remain unfinished.
See [the component qualification record](WASI_COMPONENTS.md) for exact APIs and
receipts.

Canonical guest frames now own traced slot arrays through stable native links.
Their lifetime is independent of a native handle scope, and frames can finish
out of order. A nonblocking C Promise poll preserves fulfilled values and the
original rejection in those frames without running another task's jobs. These
supply the rooting and observation foundations for suspension; the C async guest
adapter still needs task scheduling, canonical suspension and cancellation.
Native host future/stream bindings are implemented.

Pin an explicit WASI 0.3 interface version and supported Component Model
feature set. Integrate native async lifting/lowering with Perry's promise
scheduler, task suspension/resumption, futures and streams. Upgrading a host
or running TS promises in a P2 command does not implement this ABI.

Acceptance must cover concurrent imported/exported async calls, completion
without blocking other components, cancellation during I/O, backpressure,
stream end versus failure, dropped futures, and resource cleanup after
traps/unload. Run collections while component tasks are suspended to prove
their captured Perry values remain rooted. Add cross-language async
composition and a streaming HTTP example before claiming P3 support.

The internal guest task checkpoint foundation now isolates Promise/nextTick and
cleanup jobs by canonical task while preserving ALS across C entries. A separate
maintained-C fixture proves unit-stream wakeups between suspended tasks. These
are building blocks: stable async import frames, generated callbacks, typed
future/stream guest values, cancellation and cross-language Perry guest execution
remain required before removing the guest generator guards. The current
[component support notes](WASI_COMPONENTS.md) distinguish these internal checks
from supported public output.

Retain the P2 command/profile tests during adoption. Specify the minor
version: the official release table records additional Component Model
features for 0.3.1, so a generic "P3 supported" label is insufficient.
The current 0.3.0 target does not require `error-context`: the official roadmap
still lists that type among experimental candidate features.

## Sources

- [WASI releases and adopted Component Model features](https://wasi.dev/releases)
- [Preview 1 scope and runtime adoption](https://wasi.dev/releases/wasi-p1)
- [wasmi_wasi 1.1 API](https://docs.rs/wasmi_wasi/1.1.0/wasmi_wasi/)
- [WASI 0.3 async ABI and launch](https://bytecodealliance.org/articles/WASI-0.3)
- Local implementation: `crates/perry-runtime/src/wasi.rs`,
  `crates/perry-wasm-host/src/{lib,host_runtime}.rs`,
  `crates/perry-codegen/src/wasm32/abi_adapt.rs`,
  `crates/perry/src/commands/compile/link/wasi.rs`,
  `types/perry/plugin/index.d.ts`, and `scripts/wasi_run.py`.
