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

The native embedding and Rust component oracle use the lockfile-pinned
Wasmtime 48.0.5 engine. The standalone command runner is qualified separately
with Wasmtime CLI 48.0.0.

The native `perry/component` module links a separate
`libperry_component_host.a` (or `perry_component_host.lib`). Applications that
do not import this module do not acquire its Wasmtime engine. The existing
`WebAssembly.*` and `node:wasi` APIs retain their separate implementation.

Each component has its own store, resource table, memory budget, fuel budget,
and interrupt handle. There are no implicit filesystem, network, environment,
or stdio capabilities. Application imports must be supplied explicitly.
Opt-in WASI Preview 2 providers are available through `ComponentOptions.wasi`.
Application-defined resource imports use the factories described below.
`wasi:http` is not yet qualified.

### Native WASI commands

`runComponentCommand` executes a `wasi:cli/run` component once in a fresh
native store and releases its provider resources when the call ends:

```ts
import { runComponentCommand } from "perry/component";
const status = runComponentCommand(bytes, {
  fuel: 10_000_000,
  wasi: { args: ["example", "--version"], inheritStdout: true }
});
```

This entry point enables the pinned Wasmtime-WASI 48.0.5 P2 providers
(`wasi:cli@0.2.12`, including `exit-with-code`; the original `exit(result)`
operation is covered at 0.2.0). Omitted grants
have the same closed filesystem, network, environment and stdio defaults as
`wasi: {}`. Each call receives independent arguments, environment, preopens
and resource tables. It does not accept application imports. Reusable
libraries use generated WIT bindings and explicit instance lifetimes.

A returned `ok` or `err` becomes status 0 or 1. An explicit WASI exit request
returns its status without terminating the native Perry process. Invalid
components, missing/mistyped command exports, guest traps and exhausted
budgets throw `ERR_COMPONENT`. Export validation happens before component
initializers execute. Initialization and execution share one fuel budget;
fuel does not interrupt time spent in synchronous host I/O.

The combined component-host boundary uses ABI 5; compiler, runtime and
optional host archive must be built together. The separate command feature
was qualified using ABI 3; the combined Linux qualification is recorded below.
This API is native-only. Unit fixtures and
`scripts/component_command_acceptance.py` cover return/exit behavior,
initializer failures, limits and a Rust `wasm32-wasip2` command hosted by
Perry in ordinary and moving-GC modes. Linux qualification passes all five
engine tests, 20 runtime component tests and five command acceptance phases.
The moving run copies six objects, verifies six evacuation cycles and
protects retired from-space. Dynamic short strings such as `String(exitCode)`
are covered, along with closed defaults and independent capability grants.
The [execution receipt](validation/component-commands-20261021.json) records
the source revisions and artifact/log hashes. This does not establish HTTP,
P3 async, other native host platforms or I/O cancellation support.


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
[Wasmtime resource-drop implementation](https://docs.rs/wasmtime/48.0.5/src/wasmtime/runtime/component/resources/any.rs.html).

The embedding distinguishes a failed destructor after completed disposal
from a disposal rejected during an active call. Completed disposal severs
the private JavaScript import table before propagating an error; rejected
reentrant disposal retains it for the still-running component. The FFI and
weak-reference GC regressions for this distinction pass on Linux,
including a moved wrapper and collection of released callback captures.
The same distinction applies to individual resource drops: a wrapper becomes
disposed even if the guest destructor traps without invoking a JS callback.

GC finalizers release native engine memory without executing guest or
JavaScript callbacks. Native resource wrappers register deferred cleanup
jobs, qualified on Linux as recorded below. A live borrow
keeps its owned wrapper reachable. Collected owners are released at a safe
job/call boundary, with identity and generation checks protecting resources
that have been transferred or returned to a new owner. Cleanup encountered
during an active engine call waits until the call has returned. A primary
call exception takes precedence over a cleanup exception; cleanup continues
through the remaining collected identities after a destructor fails.

Unreachable whole instances still release native memory without invoking
application callbacks. Collection timing and poisoned guest destructors are
not deterministic: explicit disposal remains the reliable lifecycle API.
Cleanup of wrappers inside Perry-authored WASI guests is described below;
its Linux execution qualification is recorded below. The combined native host
ABI is 5; matching runtime and engine archives are required. The earlier
cleanup-only qualification used ABI 4.

## Resources supplied by a native Perry host

Generated `load` accepts an imports builder as well as an ordinary imports
object. The builder receives resource factories bound to the new instance;
they are usable before component initialization invokes imported constructors.

```ts
const plugin = load(bytes, (resources) => {
  const counter = resources["example:counters/api@1.0.0"].counter;
  return {
    "example:counters/api@1.0.0": {
      "[constructor]counter": (value: number) =>
        counter.create({ value }, (payload) => release(payload)),
      "[method]counter.read": (handle) =>
        (counter.value(handle) as { value: number }).value,
    },
  };
});
```

Factories retain the canonical WIT identity through `use` aliases. A handle
from another instance or another resource type is rejected. Payloads remain
GC roots while the component owns the resource; canonical representations
contain opaque integers, never Perry heap addresses. Receiving ownership back
creates a new wrapper generation, so the earlier owner and its borrows stay
invalid. Borrowed callback arguments expire when the callback returns.

Argument conversion validates the complete input before committing transfers.
Invalid later arguments, duplicate owners, and an owner borrowed in the same
call leave ownership unchanged. Host destructors remove payload roots before
calling application code and run at most once. Explicit component disposal
also cleans up host resources still held by the guest, including after a
guest trap. Cleanup continues after a destructor throws and preserves the
first JavaScript exception. Failed initialization releases payloads created
by the builder or imported constructors.

An owned argument received by an import callback may be retained or disposed.
Disposal releases its payload immediately and retires the canonical handle
as the callback returns, including when its destructor throws. This does not
permit reentrant guest calls or disposal of an owner borrowed by the active
call; those operations still pass through the store's reentry guard.

Rebuild the component-host archive and native runtime together for native host
ABI version 5. Unreachable native wrappers use the deferred cleanup described
above; explicit disposal provides deterministic release. Built-in WASI
resource providers remain controlled by `options.wasi`; application factories
do not grant access to those providers.

## Resources in Perry-authored guests

Guest bindings export `resources[interfaceName][resourceName]` factories.
Use `create(payload, destructor?)` to return an owned WIT resource and
`value(handle)` to access its payload through a live owned or borrowed handle:

```ts
import { register, resources } from "./generated/guest";
const counter = resources["example:counters/api@1.0.0"].counter;
register({
  "example:counters/api@1.0.0": {
    "[constructor]counter": (value: number) => counter.create({ value }),
    "[method]counter.read": (handle) =>
      (counter.value(handle) as { value: number }).value,
  },
});
```

Imported resource owners expose `borrow()` and `dispose()`. Passing an owner
to an `own` parameter transfers it; later use of that wrapper or its borrows
fails. All arguments are validated before any owner is consumed, including
nested records/lists and duplicate references. An owner cannot be borrowed
and transferred in the same call. Borrowed parameters expire when the call
returns, even if guest code saves their wrappers. Repeated disposal is valid
only for a wrapper that was actually disposed, not one transferred elsewhere.

Guest-defined payloads remain rooted while the canonical host owns their
resource. The canonical representation carries an opaque integer identity,
never a Perry heap address. A destructor receives the payload and runs once
when the canonical owner drops the resource; its payload root is removed
before user code runs, including when that code throws.

Unreachable owned wrappers inside the guest register deferred finalization
jobs. The holdings retain private ownership state, not the wrapper. A transfer
changes that same state, so a late job cannot drop a recycled canonical handle
belonging to a later owner. Live borrows retain their original owner; explicit
disposal unregisters the finalizer and remains idempotent. A drop retires the
private state before invoking the canonical destructor, including when it fails.

Library components perform a microtask checkpoint after each successful
synchronous export, while its arguments and result remain rooted and before
the adapter lowers the result. This delivers ready finalizers, nextTick and
Promise jobs without a command event loop. It leaves timers and immediates
pending, does not wait for I/O, and does not implement asynchronous WIT
results. Linux execution qualification of these cleanup and checkpoint paths
is recorded below.

Collection timing is nondeterministic, and payloads or destructors that retain
their own wrapper can keep it reachable. Explicit disposal provides predictable
release. Running every destructor after an instance trap is not guaranteed.

Resources from an interface imported and exported by the same world have
distinct nominal identities. Generated declarations and runtime handles keep
those identities separate. WIT `use` aliases preserve the original resource
identity and do not introduce a new guest factory. Guest adapters use ABI
version 2; rebuild the
adapter and WASI runtime together rather than mixing earlier archives.

The guest resource profile is qualified on Linux with the independent Rust
provider/caller described below.

## Acceptance work

`perry-wit --guest-c <wit-path> <output-directory> [world]` also generates
the maintained wit-bindgen C bindings, component type object, and Perry
canonical adapter. C header compilation tests cover synchronous scalar and
aggregate types, including interfaces imported and exported in the same
world, with resource handles and generated destructor glue. The WASI
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

The guest resource fixture also passes on Linux against an independent Rust
provider/caller. It checks imported ownership and explicit disposal, duplicate
transfer rollback, nested own handles, a borrowed Rust-owned resource passed
through a WIT alias, expired borrowed parameters, and 51 successful exported
resource destructions. A throwing destructor must produce the deliberate
adapter trap; fuel exhaustion cannot satisfy that check. The separate
`--guest-moving-gc` profile uses a WASI runtime built with `--gc-instruments`
and requires actual copied objects and evacuation verification. Both normal
and forced-moving runs pass. Wasm execution does not claim native from-space
memory protection. A second fixture imports and exports the same resource
interface, rejects cross-type substitutions, and passes nested ownership
transfer and destruction checks from Rust. The [execution receipt](validation/component-guest-resources-20261021.json)
records the exact source, artifact hashes and moving-GC witnesses.

The native WASI-provider profile has passed on Linux with freshly built
compiler, native runtimes and wasm32-wasip2 runtime. Native Perry hosted a
Perry-authored component for 51 typed calls in both normal and moving-GC
modes; the latter required copied objects and protected retired from-space.
Three interleaved component instances retained separate arguments,
environments and preopens. The execution checks rejected ambient environment
access, reads without preopens, writes through read-only preopens, and parent
or symlink escapes; an explicit write grant produced the expected host file.
Unit tests additionally executed a declared WASI clock import and actual TCP
bind grants/denials. That initial provider qualification did not include
UDP/DNS interoperability; the subsequent networking qualification is recorded
below. Blocking-I/O cancellation and HTTP remain unqualified. Command
execution is qualified separately above.

The subsequent networking profile passed on source
`adf7e5229147e74865dc8de76ff01ca6c988ccfb`: 23 engine and 20 runtime component
tests, plus all 21 native acceptance phases. A native Perry host executed a
Rust-authored component using the pinned WASI providers. An independent peer
observed six UDP and three TCP exchanges with exact payloads in both ordinary
and moving-GC runs. The fixture verifies separate local-bind and destination
grants, denied addresses/ports, DNS capability separation and fresh-instance
isolation. The moving network run copied 178 objects, passed six evacuation
checks and protected retired from-space. The
[networking receipt](validation/wasi-network-interop-20261021.json) records
the source, traffic and artifact identities. This does not establish UDP
support in Perry-authored WASI guests or qualify blocking-I/O cancellation,
HTTP or P3.

Host-defined resources are now qualified on Linux. On source
`286341d1caa88e9f1d26073e1423e17f1f479a7f`, all 25 engine, 13 WIT and 23
runtime tests pass, together with 21 native and 34 guest acceptance phases.
The native host-resource fixture covers ownership rollback, aliases, foreign
types, callback lifetimes, retained owners, explicit disposal and throwing
destructors. Its moving-GC run copied 1,107 objects, passed 81 evacuation
checks and protected retired from-space. The [execution receipt](validation/component-host-resources-20261021.json)
records the exact source and artifact/log hashes. That receipt predates
deferred cleanup and does not qualify the new finalization path.

Native deferred cleanup is qualified on source
`e510a0c9da3763ba0202aef9c25efba1b9b65d59`: all 21 native and 34 guest
acceptance phases pass, including unreachable owners, live borrows, transferred
handles, renewed ownership and exactly-once destruction. The host-resource
stress run moved 1,893 objects and passed 97 evacuation checks with retired
from-space protected. Runtime component tests (26), weak-reference tests (6)
and the FinalizationRegistry test filter (6) pass. The 28 engine tests passed
on an earlier revision with byte-identical engine and ABI sources.

The finalization delivery regression installs production root scanners and
forces collection from each async init hook. The old implementation delivers
only one of three jobs; the fixed implementation delivers all three and
retains their identities. The [cleanup receipt](validation/component-cleanup-20261021.json)
records both source revisions, acceptance reports and artifact/log hashes.

Guest deferred cleanup is qualified on source
`23b5c3aadc18237488d6c978ece554483d774c78`: all 28 runtime component tests,
21 native acceptance phases and 34 guest acceptance phases pass. Independent
Rust-host execution covers abandoned imported/local owners, live borrows,
transfer, explicit disposal, rooted return values and exactly-once destruction.
The guest resource run copied 5,225 objects and passed 217 evacuation checks;
WASI has no native from-space page protection. Native stress runs also observed
quarantined from-space.

The checkpoint regression fails against the old implementation because it
runs an immediate during the export checkpoint. The fixed implementation runs
only the microtask, retains the returned value across an observed move during
that checkpoint, and leaves the immediate for explicit later dispatch. The
[guest cleanup receipt](validation/component-guest-cleanup-20261021.json)
records both revisions and the matching artifact/log identities.

The combined profile passed on source
`b5fb0b6df903e491df1182150672f0d769b403d8`: 40 engine, 13 WIT and 29 runtime
component tests, plus six weak-reference and six finalization regressions.
All 28 native, nine command and 34 guest acceptance phases pass against
matching compiler, engine and native/WASI runtime artifacts. This combines
commands, host resources, native/guest cleanup and networking under native
ABI 5 while keeping guest adapter ABI 2.

A new integration fixture invokes a command from a live library callback,
checks explicit exit status and library survival after command traps, then
runs another command after library disposal. Its moving-GC run copied 53
objects and passed 15 evacuation checks with retired from-space protected.
The guest resource run copied 5,225 objects and passed 217 evacuation checks.
The [combined receipt](validation/component-integration-20261021.json)
records exact phase sets, all eight moving-GC witnesses and artifact/log
identities. The required component gate now also runs the full `weakref::`
and `finreg` test selections; an earlier narrow filter is not counted as a
substitute for those regressions.

Still required before claiming the full milestone:

- Qualify the remaining capability operations,
  HTTP and blocking-I/O cancellation independently.
- Qualify supported host platforms. [PR #12985](https://github.com/PerryTS/perry/pull/12985)
  added the component acceptance suite to required execution checks; integration
  with that workflow exercises these extended resource fixtures.


P3 async support remains a subsequent milestone.

## Continuous execution checks

`scripts/component_check.sh` builds matching native tools, ordinary and
instrumented runtime archives, and the WASI runtime, then runs both acceptance
harnesses. Configure wasi-sdk first, as for the standalone WASI checks. Fixture
Cargo builds run from the checkout so they use its pinned Rust toolchain and
unwind configuration; `CARGO_BUILD_JOBS` is respected.

The Linux `wasi-execution` job invokes this script after the standalone suites
in the core PR, sweep and full tiers. Its result feeds the required `pr-gate`;
a failed or skipped planned execution blocks the gate. Phase logs, JSON
verdicts, component artifacts and the source revision are retained even on
failure. Docs-only PRs skip this build. The extended `wasi-check / wasi-codegen`
recipe also retains component execution on Linux; its wider platform matrix
does not yet qualify native component hosting outside Linux.

The combined required job passed in
[run 39614047391](https://github.com/PerryTS/perry/actions/runs/39614047391/job/119271501754)
on PR head `dfd880f2fae16bba04c99ce354417252331fa8dd`, tested as merge revision
`53fab4a9450f9d5909654f4dc4d713971085ffe3`. Its retained reports pass all 36
standalone command cases, 14 native component phases, and 34 guest component
phases. Native moving runs recorded copied objects and protected from-space;
the WASI resource run recorded 206 verified collections and 4,579 copied
objects. The [receipt](validation/component-required-ci-20261021.json) records
source revisions, artifact digest and phase names. This qualifies promotion
of the existing Linux component profile; host-defined resources and native
command hosting remain separate changes.
