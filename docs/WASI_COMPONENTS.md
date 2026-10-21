# Typed components: implementation and qualification

This work implements an optional native component host and WIT bindings.
The complete bidirectional milestone in [WASI_ROADMAP.md](WASI_ROADMAP.md)
is still in progress. A generated TypeScript guest module alone does not
establish that Perry can emit a reusable component.

## Native host

`perry-wit <wit-path> <output-directory> [world]` resolves the selected WIT
world and writes `bindings.ts`, `guest.ts`, and `world.json`. Native callers
import `load` from `bindings.ts`; the generated `exports` object has the WIT
function signatures. Interface identifiers retain their package versions.

The native `perry/component` module links a separate
`libperry_component_host.a` (or `perry_component_host.lib`). Applications that
do not import this module do not acquire its Wasmtime engine. The existing
`WebAssembly.*` and `node:wasi` APIs retain their separate implementation.

Each component has its own store, resource table, memory budget, fuel budget,
and interrupt handle. There are no implicit filesystem, network, environment,
or stdio capabilities. Application imports must be supplied explicitly.
Opt-in WASI Preview 2 providers are available through `ComponentOptions.wasi`.
General host-defined resource imports remain unimplemented; command worlds
and `wasi:http` are not yet qualified.

### Per-instance WASI providers

```ts
const plugin = load(bytes, applicationImports, {
  wasi: {
    args: ["plugin", "--mode=preview"],
    env: { MODE: "preview" },
    preopens: [{ hostPath: "./data", guestPath: "/data" }],
  },
});
```

Omitting `wasi` links no built-in WASI providers. `wasi: {}` enables clocks
and random numbers, with closed stdin, discarded stdout/stderr, no arguments,
no environment variables, no preopened directories and no network access.
Each stream can explicitly inherit its host counterpart with `inheritStdin`,
`inheritStdout` or `inheritStderr`. Arguments and environment are copied from
options; there is no ambient environment inheritance option.

Each preopen defaults to read-only. `readOnly: false` explicitly permits
writes. Paths are resolved inside Wasmtime's directory capabilities, including
symlink and parent traversal checks. Permissions apply to the host directory
at component creation; a nonexistent host directory fails the load.

Network access requires both a protocol grant (`network.tcp` or `network.udp`)
and a matching entry in `network.addresses`. Entries contain an IP literal
and optional port; omitting the port explicitly grants all ports on that IP.
These grants cover binds and connections. `network.dns` separately enables
name resolution and does not authorize access to resolved addresses. All
three switches default to false, and the address list defaults to empty.
This is a synchronous host: fuel and epoch interruption bound guest execution,
not time spent in blocking host I/O. Host I/O cancellation remains future work.

With providers enabled, the `wasi:` import namespace is provider-owned;
application callbacks cannot override it. Generated `ApplicationImports`
omits those interfaces while preserving required custom functions. Unsupported
interfaces still fail instantiation; enabling WASI does not enable `wasi:http`.
The provider dependency is pinned to Wasmtime-WASI 48.0.5. This implements P2
interfaces, not the P3 native async component ABI.

Flag declarations are limited to 1–32 labels, matching the
[Component Model binary format](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md#type-definitions).

The value mapping uses BigInt for `s64/u64`, Uint8Array for `list<u8>`, and
tagged objects for options, results, and variants. In particular,
`some(none)` stays distinct from `none`. Resource wrappers retain private
ownership tokens; application objects cannot forge a token by adding public
properties. Async functions, futures, and streams require the future P3
profile and are rejected by this P2 generator.

## Disposal and traps

Dispose owned resources and component instances explicitly. Successful
disposal is idempotent. Transferred resources and disposed instances cannot
be reused. A resource borrowed into a callback is valid only for that call;
a borrow of a host-held owner also depends on that owner remaining live.

A guest trap or a failing host import is distinct from a WIT `result::err`.
The Wasmtime synchronous store becomes poisoned after a trap and forbids
further guest execution, including guest destructors. Disposal still releases
store memory and callback captures, and reports the original failure. It
cannot promise execution of every guest destructor after a trap. See the
[Wasmtime resource-drop implementation](https://docs.rs/wasmtime/48.0.0/src/wasmtime/runtime/component/resources/any.rs.html).

The embedding distinguishes a failed destructor after completed disposal
from a disposal rejected during an active call. Completed disposal severs
the private JavaScript import table before propagating an error; rejected
reentrant disposal retains it for the still-running component. The FFI and
weak-reference GC regressions for this distinction pass on Linux,
including a moved wrapper and collection of released callback captures.
The same distinction applies to individual resource drops: a wrapper becomes
disposed even if the guest destructor traps without invoking a JS callback.

GC finalizers release native engine memory without executing guest or
JavaScript callbacks. Deferred guest-resource cleanup is still outstanding;
GC is not a substitute for explicit disposal in this implementation.

## Acceptance work

`perry-wit --guest-c <wit-path> <output-directory> [world]` also generates
the maintained wit-bindgen C bindings, component type object, and Perry
canonical adapter. C header compilation tests cover synchronous scalar and
aggregate types, including interfaces imported and exported in the same
world. Guest resource adapters are diagnosed as unimplemented. The WASI
static-library route and canonical guest adapters have passed the
transform fixture on Linux with matching compiler and runtime artifacts.

`scripts/component_host_acceptance.py` builds the Rust transform fixture,
componentizes it, generates WIT bindings, compiles its native Perry caller,
and checks 51 calls with strings, byte lists, typed errors, callback GC,
and explicit disposal. It requires matching prebuilt tools and runtime
archives; it records each phase and its real exit status in `results.json`.
Runtime unit tests separately assert that adapter values actually move
during collection.

The native harness also includes a Rust resource fixture covering borrowed
access, consumed owners, duplicate transfer rejection, repeated disposal,
and separate guest-trap and JS-exception destructor paths. This arm has
passed on Linux in normal and instrumented moving-GC modes; the latter
observed six copying collections and protected retired from-space.

The native transform fixture has passed all 51 calls on Linux. The separate
`--moving-gc` arm requires instrumented archives, a positive copied-object
count, and a from-space quarantine witness. It has also passed on Linux: 51
typed calls completed with 101 copying collections and protected retired
from-space.

`scripts/component_guest_acceptance.py` generates the canonical guest C
adapter, compiles the TypeScript static library, and links a P2 reactor with
wasi-sdk. A typed Rust host checks the same 51 calls. It then connects the
Perry library's logger import to a Rust component using
[WAC's composition graph](https://github.com/bytecodealliance/wac). The Rust
logger adds a prefix to an audit import; the host checks that prefix and
the call count, and supplies no direct logger import. This checks execution
through both components. Both the direct Rust-host run and composed run
have passed on Linux, with
51 typed calls in each run. A further all-values fixture checks integer
limits, nested options, 32 flags, tuples, variants and void/scalar calls
through an interface used as both an import and an export. All 14 calls
pass against the independent Rust oracle, including imports invoked from
Perry and values returned after guest collection.

Still required before claiming the full milestone:

- Implement guest resource adapters and qualify ownership and destruction
  in Perry-authored components.
- Implement host-defined resource imports and deferred cleanup, and qualify
  capability providers, command worlds, and HTTP independently.
- Qualify supported host platforms and promote the demonstrated execution
  check into the required PR checks.

P3 async support remains a subsequent milestone.

## Continuous execution checks

`scripts/component_check.sh` builds matching native tools, ordinary and
instrumented runtime archives, and the WASI runtime, then runs both acceptance
harnesses. Configure wasi-sdk first, as for the standalone WASI checks. Fixture
Cargo builds run from the checkout so they use its pinned Rust toolchain and
unwind configuration; `CARGO_BUILD_JOBS` is respected.

The Linux `wasi-check / wasi-codegen` CI job invokes this script after the
standalone suites. It retains phase logs, JSON verdicts, component artifacts
and the source revision. The extended-suite routing and PR-label condition
still apply; this check is not yet required on every relevant PR.
