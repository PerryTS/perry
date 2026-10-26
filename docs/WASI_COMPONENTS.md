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
Wasmtime 48.0.7 engine. The standalone command runner is qualified separately
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
`wasi:http` client imports are implemented; their Linux engine and canonical
component qualification is described below. Incoming HTTP handling and authored
HTTP components are qualified on Linux below; native WASI 0.3 remains in progress.

### Native async functions

WIT `async func` declarations use the canonical async calling convention.
The manifest preserves this bit and the engine checks it before instantiation;
a synchronous manifest cannot silently call an async export. Generated native
TypeScript bindings return `Promise<T>` and accept Promise-returning async
imports. Synchronous declarations and legacy manifests keep their existing
calling convention. The native Rust embedding exposes `Host::call_async` and
`Host::new_with_async_imports`.

Generated standalone component bindings use the persistent driver described
below and permit concurrent async invocations. The legacy TypeScript path
permits one pending async invocation per component and rejects a second until
the first settles. Independent components can progress through the same agent
event pump. Native I/O wakes that agent using its identifier.
Only the owning thread executes guest code, JavaScript imports or cleanup.
Pending operation payloads trace the component, result Promise and original
JavaScript rejection. Argument values are snapshotted at submission, and resource
wrappers remain traced while queued and while async import results are lowered.
Snapshots read declared WIT record fields and selected tagged payloads; extra
getters and void result values are not inspected. Pending imports keep their
result schema in private traced state. Retained wrappers carry identities saved
at encoding time under a fixed traced holder, so later matching does not
re-encode a transferred host wrapper or assume JSON field order matches root
order. The persistent path consumes native ownership receipts as described below.
Import arguments cross the engine as owned JSON values,
and borrowed host resources remain valid until their import Promise settles.
Successful calls retain the Store for reuse. Traps, rejected imports and explicit
disposal cancel suspended guest tasks and retire that component's Store.

The Rust embedding can opt into `Host::into_driver()`, which returns an
owner-thread `Driver` future and a cloneable `DriverClient`. Poll the driver
while awaiting calls submitted with `client.call(interface, function, args)`.
The driver accepts concurrent calls and continues polling guest imports after an
export publishes its result. `Driver::is_active()` distinguishes guest work
from an idle mailbox. `current_invocation()` identifies the original call in
synchronous and asynchronous callbacks, including later polls and cancelled import cleanup.
It contains no JavaScript value or callback frame. Overlapping and post-return
tasks share one fuel budget, refreshed only after quiescence. The mailbox bounds
outstanding requests, including queued requests whose futures were dropped.
Dropping a call before execution skips it; dropping an unconsumed call after it
starts requests Store retirement. Dropping the last client returns the Host if
quiescent and retires it otherwise. The TypeScript bridge routes standalone
public calls and callbacks through this driver.

`DriverClient::retained_origins()` reports native invocation IDs whose callback
context may still be needed. It combines queued/unpublished requests, guest-root
bindings and leases held by captured host futures. Reused engine task IDs replace
old bindings, so completed calls can release their context while older background
work continues. With `Host::new_for_driver`, locally observable callback exports
also report validated task exit through a private wrapper. Other layouts retain
conservative bindings until their IDs are reused or the Store becomes quiescent.
Embeddings must also retain context for results they have not consumed.
Store retirement clears guest roots; captured callback context remains available
until future cleanup finishes, including futures dropped before their first poll.

The [native context-retention receipt](validation/wasi-native-context-retention-20261025.json)
records 142 engine and 22 WIT tests in development and production panic-abort,
a 512-call workload with older background work still active, and independently
failing controls for context accumulation and premature cleanup. The registry
contains IDs and counts only. That earlier receipt qualifies the Rust boundary;
the later persistent-runtime receipts below cover TypeScript frames, ALS
snapshots, error routing and wrapper GC.

The Rust driver transports generic `future<T>` and `stream<T>` values as owned,
Store-local JSON descriptors. Use `client.read_async_value(&value, capacity)`
and `client.close_async_value(&value)`, or transfer a descriptor as an exported
function argument. Futures yield one payload (`None` for void). Streams yield
`{done:false,value:[...]}` chunks, including a final partial chunk, followed by
`{done:true}`. Unit streams preserve element counts with null placeholders.
Only one read may be pending per descriptor. Capacity is bounded to 65,536
elements, and the Store permits at most 1,024 live descriptors. Exact instantiated
payload and resource identities are checked before ownership transfers;
foreign, duplicate and incompatible transfers fail before entering the guest.

Closing a pending read cancels it without retiring the Store. A close accepted
before the read commits prevents publication and destroys any copied owned
payload exactly once. An accepted close still executes if its result observer
is dropped. Closing an unread value wakes its producer. Components with generic
async values require the persistent Rust driver; legacy `call` and `call_async`
reject them before executing an export.

Rust producers discover `AsyncValueType` handles through
`client.async_value_types()`. Each handle belongs to its component and preserves
the selected world's WIT payload aliases. `client.create_async_value(&ty)`
returns `{value,writer}` with separate owned descriptors. Use
`client.write_async_value(&writer, values)` for bounded writes; a future takes
one element and void/unit payloads use null placeholders. The result is
`{written,remaining,cancelled,closed}`. Partial writes return the unconsumed
suffix; after canonical transfer, its owned resources and nested async values
receive fresh descriptors. Preflight failures preserve the original owners.

`cancel_async_write` targets writes registered when cancellation was submitted
and returns their remaining ownership through each write's result. It retains
an unfinished writer for retry. `close_async_writer` closes a stream, cancels
pending writes and disposes unconsumed owners. Accepted cancellation and close
commands execute even if their observers are dropped. A future finishes by
writing its one value; `abort_async_writer` is the explicit producer failure
path and retires that Store, including sibling work. An ordinary WIT `result`
payload can carry an application error without retiring the component.

The quota counts both readable and writable descriptors, including reservations
for factories in progress. Writer handles survive quiescent Host recovery and
restarting its driver. The [native producer receipt](validation/wasi-native-producers-20261025.json)
records 138 engine and 22 WIT tests in development and production panic-abort,
real guest reads through imports and exports, and failing controls for late
cancellation and close races. Native TypeScript wrappers and producer interfaces
have the separate qualification below.

The versioned C boundary now also exposes a persistent driver. Construct it
with `perry_component_driver_new`, discover local factory indices with
`perry_component_driver_types`, and submit JSON commands through
`perry_component_driver_submit`. Supported operations are `call`, `read`,
`close`, `collect`, `create`, `write`, `cancel-write`, `close-writer`,
`abort-writer`, `collect-writer`, `resource-drop` and `resource-collect`.
Submission snapshots native JSON and returns a decimal-string invocation ID
without executing Wasm. Queue-only submission is permitted from a callback;
recursive execution is rejected. A submission requires an output buffer so its
accepted invocation ID cannot be lost.

`perry_component_driver_poll` runs on the owner thread with fresh primary and
cleanup context tokens and a native notification callback. Imports and lifecycle
callbacks receive their originating invocation ID explicitly. Origin zero means
initialization or component-wide retirement. All persistent instances use the
embedding-stack dispatcher, including worlds with only synchronous WIT functions.
Poll returns `completed`, conservative `retained` IDs, `active`, `retired`, an
optional driver `error`, and ordered, deduplicated `failedOrigins`. Async import
rejections keep their origin until reported; retirement uses the cleanup context.
Consume completed results before trimming context records. Invocation IDs are
strings in JSON to preserve all 64 bits in JavaScript.

`retiredByWriterCollection` identifies an unfinished future writer whose
collection intentionally retired the Store. Match this origin to an admitted
writer collector before suppressing the generic retirement summary. Pending
public operations still reject, and actual callback/destructor errors still
propagate. An unrelated trap or unmatched marker is not intentional disposal.

Schedule an initial poll after constructing a persistent driver, even before
submitting an export call. A core start can enqueue an async import which runs
on that first poll at origin zero. Outstanding embedding reply channels keep
the C driver `active`, including when Wasmtime considers the unobserved subtask
quiescent. Resolving the import allows it to become idle; rejection retires the
Store and reports failed origin zero. Preserve initialization context separately
from the nonzero invocation IDs in `retained`.

`perry_component_driver_cancel` discards an observer. Before execution it skips
the request; after execution starts it retires the Store on the next poll.
Accepted soft cancellation and close commands still execute. Explicit
`perry_component_host_dispose` retires a driver and reports `RELEASED_ERROR`
when its embedding destructor fails. Native `host_drop` does not invoke embedding
callbacks. Legacy execution, resource and HTTP entries reject persistent handles;
typed resource cleanup and supported P3 HTTP handlers have driver commands. The
TypeScript runtime now uses the persistent driver for standalone component
worlds, including worlds with synchronous functions. P3 HTTP handlers with a
locally observable callback ABI also use it. P2 incoming handlers and nested or
imported P3 handler implementations retain their serialized entry points. The
HTTP profile is selected before initialization, so fallback does not execute
initializers twice. The current boundary requires matching ABI 12
runtime and engine archives. The earlier C-only qualification used ABI 9; see the
[persistent C driver receipt](validation/wasi-persistent-driver-ffi-20261025.json).
Native future/stream readers, producers and their GC cleanup now use this driver;
async guest adapters remain unfinished. The
[public runtime receipt](validation/wasi-persistent-runtime-20261026.json)
records 66 component and 12 async-hooks tests in development and production,
plus compiled production hosts proving reverse completion, original rejection
identity and collector cancellation with verified moving GC. Command and plain/TLS
HTTP regressions also pass with protected from-space pages.

The runtime can execute a saved Promise context after the public Promise is
collected. Captured ALS snapshots and observed async resource-state cells need
separate traced edges; opaque execution IDs alone do not keep metadata alive.
The [captured-context checks](validation/wasi-captured-context-20261025.json)
cover actual moving collection, success, throw and caller-context restoration.
The persistent runtime uses this primitive for origin-specific callbacks.

The existing async path now uses poll-local batches of stable callback frames.
Frames created during another callback keep their values in private traced array
rows: appending an outer-scope runtime handle during a nested callback would
lose that root when the nested scope returns. Frames preserve separate captured
contexts and first exceptions, and cleanup can enter saved ALS without retrying
a failing before hook. The persistent runtime reconstructs these frames from
managed invocation records for each poll.

Pending operations now own saved context records between polls. The records
trace snapshots and observed metadata; native fields contain public serial IDs.
A saved record can reconstruct a fresh poll frame after earlier frames and
the public Promise have been collected. This separates context retention
from public-result retention. Persistent runtime records own each invocation's
Promise, argument roots, saved context, first error and uncommitted import
returns. All native-ready results are staged before settlement hooks run, so
nested disposal cannot erase another completed result. Calls admitted by a
settlement hook survive the older retention snapshot. Idle drivers release
their native owner pin; active background work keeps only its required records.

Construct Rust hosts with `Host::new_for_driver` before `into_driver` to
prepare resource disposal. The C driver constructor selects this path. It
composes one private helper with the original nominal resource aliases and
resolves its disposal functions to actual instantiated resource types. The
helper uses no linear memory and adds one core instance when resources are
present; it shares the configured instance/fuel limits. Legacy constructors
retain their previous composition and limits. No invented future/stream factory
types are exposed for ordinary resource worlds.

`client.drop_resource(&descriptor)` and the C `resource-drop` command take a
complete owned descriptor (`value`), distinguishing guest from host resources
and validating host generation. An invalid or borrowed descriptor rejects
before execution. A trapping destructor retires the Store. Accepted disposal
still runs if its observer is discarded. `client.collect_resource(&request)`
and C `resource-collect` use the existing typed collector request and ignore
stale or transferred ownership. Guest and host tokens with identical text
remain distinct. Both operations run canonical destructors within the
persistent Store loop, with callbacks dispatched on the owning embedding stack.

Export argument borrow guards prevent concurrent owned transfer or explicit
disposal of a borrowed owner. Deferred collection waits for every active borrower,
then revalidates ownership before disposal. The guard ends when the exported
result is published: Wasmtime returns canonical resource borrows at `task.return`.
It does not wait for background task exit. The native registry contains token
strings, counts and native wakers only. Cancellation removes waiting collectors
and guards during Store retirement.

Async import arguments have separate borrow scopes. A resource loaned to an
async application import can be forwarded through the driver's queue to another
export while that import is pending. Submission pins the native alias before
execution, including when the import returns immediately after submitting work.
Settlement rejects new forwarding and waits for already accepted borrowers.
Completing one import, or calling a synchronous import, does not remove another
import's aliases. Expired, foreign and ownership-changing borrow descriptors
reject before entering the guest. Synchronous import loans cannot outlive their
callback and cannot be submitted as queued forwarding work.

This applies to JavaScript-backed host resources and provider resources; the
native C tests forward an actual WASI network resource. Borrow aliases use the
maintained async cleanup API without retaining a Store borrow across polls.
The runtime codec keeps their native capability and expiration lifetime in
private GC-traced state. The persistent runtime now routes these descriptors
through its queued call boundary; cross-language and platform qualification
remain separate requirements.

Composition uses the pinned `wac-graph` 0.12.0 source with a local nominal
resource alias fix. Freestanding functions can borrow or own imported resources,
including anonymous interfaces, identical resource names and nested signatures
that reference outer resources without re-exporting them. The unchanged upstream
composition fixtures also pass. See [vendor provenance](../third_party/wac-graph/VENDORED.md)
and the [borrow interop receipt](validation/wasi-import-borrows-20261025.json).

Persistent C drivers also extend the existing `lower` lifecycle event with
optional `guestTransferred` and `asyncTransferred` token arrays. The first uses
the opaque `$resource` namespace, including provider resources; the second covers
owned futures and streams. Host `created`/`transferred` references keep their
existing shape. One callback validates the whole transaction before native
ownership changes; rejecting it leaves the original capabilities usable. A
later invalid aggregate member publishes no event. Native-only clients may omit
the lifecycle callback, and the public Rust `ResourceEvent` contract is unchanged.

The runtime can use these identities to release only the corresponding retained
import-result wrappers. Resolving an import channel or finishing one poll does
not prove ownership committed: the import may still be waiting for accepted
borrowers. The native test returns an owned WASI resource while forwarding a
borrowed one and verifies the commit follows the borrower under the original
invocation. See the [ownership receipt qualification](validation/wasi-ownership-receipts-20261025.json).
The persistent TypeScript runtime consumes these notifications only after
validating the host ownership transaction. Saved identities match every retained
alias without re-encoding a wrapper after transfer. Channel acknowledgement
releases the pending import Promise; the response root plan remains until the
actual native ownership receipt.

For readable future/stream wrappers, `client.collect_async_value(&value)` and
the C `collect` command ignore descriptors already consumed, closed or transferred.
Live pending reads use the same cancellation protocol as explicit `close`.
This keeps a late GC request from taking ownership back from a guest or a newer
wrapper. Explicit close retains its validation errors. The
[persistent cleanup receipt](validation/wasi-persistent-cleanup-20261025.json)
records the native boundary checks. Traced TypeScript readers now use this
stale-safe collector for explicit disposal and GC cancellation.

This is a subset of the WASI 0.3 milestone. Enabling `wasi` now links the
Turnloop-backed 0.3.0 providers as well as the P2 providers. WIT manifests
and generated TypeScript declarations now preserve `future<T>` and `stream<T>`,
including nested, payload-free and byte-stream types. Native application handles
are owned and nonthenable. `future.read()` consumes one typed value;
`stream.read(maxElements)` returns a bounded chunk followed by `{done: true}`.
Byte chunks use `Uint8Array`. Capacities are integers from 1 through 65,536;
the default stream capacity is 4,096. A second future read, concurrent reads on
one handle, reads after EOF/disposal, cross-component transfer, and transfer
while reading are rejected. `dispose()` is idempotent and cancels a pending read
without retiring the Store. A completed future or transferred reader cannot be
reused.

An observed read retains its temporary handle across `await` and moving GC.
An unobserved public read Promise stays weak: collecting it closes its pending
read even if the handle remains live. Collecting an unread handle closes its
producer. Ownership receipts invalidate every retained alias on transfer.
Ready owned results refused by a hook or left without an observer are collected
directly. The [reader receipt](validation/wasi-component-readers-20261026.json)
qualifies compiled Linux production hosts with streams, nested futures, a
3 MiB Unicode/byte/resource payload, suspended-producer cancellation, component
reuse, actual evacuation and protected from-space.

Standalone generated components expose `asyncTypes()`. Each component-bound
factory reports its kind and a serialized structural WIT schema, expanding
structural aliases while preserving nominal resource keys. `create<T, Chunk>()`
returns a Promise for `{kind, value, writer}`. Its original schema and native
factory index stay private; changing the public metadata cannot change the
created type. Type arguments describe the selected schema; native writes still
validate its actual WIT type and reject foreign resource identities.

A future writer accepts one `write(value)`; a stream writer accepts a bounded
`write(chunk)`. Each returns `{written, remaining, cancelled, closed}`. Byte
streams use `Uint8Array` for chunks and remainders. Payload-free futures read as
`undefined`; unit streams carry dense arrays of counted `undefined` elements.
Only one write may be pending per writer. `cancel()` cancels the current write
and preserves its unconsumed values for retry. After lowering, returned owners
have new identities and the original owners cannot be reused. Ownership receipts
release queued payload roots as soon as the native transaction commits.

`close()`/`dispose()` on a stream writer closes its readable endpoint and
disposes unconsumed pending payloads. Disposing an unfinished future writer
retires its component quietly: an unwritten canonical future cannot simply drop its
writable end. GC uses the same policy for abandoned writers or unobserved
pending writes. Stale finalizers recheck native ownership and do not abort a
completed future or affect a transferred/newer capability. Observe writes that
must remain pending; await their result to recover partial/cancelled remainders.
The [producer receipt](validation/wasi-component-producers-20261026.json)
records actual guest consumption, nominal resource ownership, nested factories,
partial writes, cancellation, GC and Store reuse with matching production
archives. Native admission shares the engine's 64 pending-call limit; up to
1,024 separately retained invocation contexts can outlive their public results.
`stream<char>` is explicitly rejected; use bytes with an explicit encoding.
`error-context` remains unsupported. Async Perry-authored guest functions and
future/stream guest values still require the rooted canonical adapter and are
explicitly rejected by `perry-wit --guest-c`. The TypeScript
HTTP bridge and Rust HTTP embedding use the separately qualified typed entry points
below. Do not infer complete P3 support from async function support or the Rust provider receipts.

The existing C frame API now retains values through frame-owned traced arrays,
using stable native links instead of retaining a transient root scope. Frames
can outlive their creating scope and finish in any order, while the collector
can still move their values. Array growth publishes its replacement head;
exceptions remain rooted; a late frame drop after heap teardown touches only
native state. The frame stays on its creating agent. This is the foundation
for async guest adapters, not an implementation of those adapters.
The [owned frame receipt](validation/wasi-owned-guest-frames-20261025.json)
records 45 runtime tests, both failing controls against the previous frame
implementation, 35 Turnloop-based WASI guest acceptance phases, and all eight
plain/TLS combinations of native and guest moving GC. It also preserves
separate future/stream architecture prototypes; those results do not qualify
production bindings.

Internal task frames now carry a native job-ownership lease and a traced ALS
snapshot. Their C call and checkpoint functions restore all Rust scopes before
returning. Promise jobs, nextTicks and deferred cleanup jobs owned by another
live task stay parked; settling a Promise from a sibling marks its owner ready.
User-restored ALS snapshots cannot change the canonical task executing JS.
Unowned jobs, and jobs whose owner has finished, can run on the next entering
task. An uncaught job error remains rooted in its task frame instead of exiting
the embedding process. Error recovery also restores the microtask runner’s native
counters and activation boundaries before C returns. Task checkpoints do not drive timers or foreign native
I/O. This internal API does not supply canonical suspension, cancellation or
async import/export adapters. See the
[task checkpoint receipt](validation/wasi-guest-task-checkpoints-20261026.json).

Canonical import callbacks now receive a stable heap frame. An internal C
`import_begin` operation retains its Promise and captured ALS in that frame;
the runtime hands the frame to C after the import callback returns. This also
applies when the result is available immediately: C must wait for the handoff
before dropping the frame. Synchronous imports still release their frame when
the callback finishes. A later `task_resume` operation runs a finite C completion
callback under its parent task, and import settlement checks that the original
task is still live. The callback operates on a distinct import frame and returns
before canonical suspension. It cannot reenter or drop its parent frame.

Resolution uses the existing Promise resolution algorithm, including thenable
assimilation, without draining jobs. Duplicate completion and invalid slots keep
the first outcome/error; an expired or foreign task cannot settle the import.
C must retain typed argument/result buffers until canonical use ends, then drop
the frame exactly once. These runtime operations do not implement that generated
C scheduler or its cancellation policy. The
[deferred import receipt](validation/wasi-guest-import-frames-20261026.json)
records native runtime tests, a WASI archive with the new symbols, and fresh
synchronous Perry guest execution from Rust, including verified moving GC and
resource cleanup. It does not qualify generated async guest execution.

A separate C-only acceptance fixture uses the maintained `wit-bindgen-c` backend
to prove cross-task wakeups through a unit stream and a canonical waitable set.
It checks reverse completion order, coalesced wakeups before and after arming a
read, and loading the component against a public world that omits its harmless
stream-type anchor export. Run `scripts/component_scheduler_wake_acceptance.py`
with wasi-sdk and a Cargo target directory. This fixture has no Perry JS runtime;
it does not qualify generated Perry async adapters or cooperative cancellation.
The [wake receipt](validation/wasi-canonical-wake-20261026.json) records its exact
source and toolchain. Public async guest generation remains guarded.

The internal generic-reader checks additionally cover canonical layouts from
`wit-parser::SizeAlign`, shared metadata for repeated aliases, a reusable
quota-limited Wasm allocator, and task-local frames. They exercise concurrent
reads and baseline cancellation, Unicode and 3 MiB compound values, final stream
chunks, and nominal resource identity. The helper is now composed into production
Rust hosts, with private read/disposal functions sharing the original component's
resource identities. End-to-end checks cover imported and exported resources,
pruned provider imports, nested futures, large compound payloads, transactional
transfers and close races. Traced native TypeScript wrappers and producer
interfaces now use the persistent driver described above.

Private canonical factory/write helpers additionally create readable/writable
pairs, perform bounded writes with backpressure, and return unconsumed payloads.
Writers use a separate helper instance from readers: Wasmtime rejects reads and
writes within one instance for nonnumeric payloads. Both instances retain the
original resource identities and share the Store's limits. Checks cover void,
wide integers, Unicode, compound bytes, partial streams, unit counts, cancellation
and retry, peer closure, and exactly-once disposal of consumed and unconsumed
owners. Native producer APIs wrap these helpers with owned descriptors; the
private writer integers are not application handles. See the
[canonical writer receipt](validation/wasi-generic-canonical-writers-20261025.json).

The standalone Rust guest acceptance host also uses `perry-wasi` and Turnloop,
with Wasmtime 48.0.7. The Tokio gate checks its separate workspace, lockfile and
source as well as the main workspace. Qualification may pass an explicit
`--cargo-config` for immutable provider sources; that does not establish that
the required Turnloop changes have been released or qualify final packages.

`error-context` is still explicitly rejected. It is an experimental
[candidate feature](https://wasi.dev/roadmap#wasi-03), separate from the async
lift/lower, future and stream features required by the
[0.3.0 adoption table](https://wasi.dev/releases#component-model-features).

`scripts/component_async_acceptance.py` exercises a compiled TypeScript host
against a canonical async component, including a suspended JS import, Store
reuse, original rejection identity, cancellation, ALS and explicit collection.
The native host/runtime boundary uses ABI 11; rebuild the matching compiler,
runtime and optional engine archives together. Other host platforms and final
distribution packages still require qualification.

The [Linux qualification receipt](validation/wasi-native-async-functions-20261025.json)
records 57 native engine tests in development and production panic-abort, 39
runtime component tests and strict TypeScript checking. The actual compiled
TypeScript host moves 3,858 objects across 19 verified copying collections with
protected from-space. It also exercises canonical resource destruction and a
throwing destructor during import rejection. Matching ABI 7 archives retain
all 28 P2 library, nine command and eight HTTP regression phases, including
moving-GC execution.

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

This entry point enables Perry's Turnloop-backed P2 and P3 providers. It
accepts the pinned async `wasi:cli/run@0.3.0` contract and the existing P2
command contract (`wasi:cli@0.2.12`, including `exit-with-code`; the original
`exit(result)` operation is covered at 0.2.0). Other P3 interface versions are
not assumed compatible. Omitted grants
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

The TypeScript API is synchronous and returns a number after execution ends.
P3 providers suspend their canonical tasks internally; this API does not return
a Promise or keep the JavaScript event loop responsive while a command runs.
Rust embeddings can await `perry_component_host::run_command_async`; dropping
that future cancels suspended guest tasks before releasing provider resources.
P2 provider I/O remains synchronous in either Rust entry point.

`scripts/component_p3_command_acceptance.py` exercises a compiled TypeScript
host with P3 return and exit codes, initializer exits, signature/version
validation before initialization, real clock suspension, closed defaults and
exact binary stdin/stdout. The stdio component waits for both stream completion
futures before reporting success. The [P3 command receipt](validation/wasi-p3-commands-20261025.json)
records the separate development, production panic-abort and moving-GC checks.
This qualification covers Linux; other native platforms and final packages
remain open.

The combined component-host boundary uses ABI 11; compiler, runtime and
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
the source revisions and artifact/log hashes. That historical P2 receipt does
not establish HTTP, P3 async, other native host platforms or I/O cancellation
support; see the newer P3 receipt above.


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
writes. Paths use the capability primitives adapted from Wasmtime 48.0.5, including
symlink and parent traversal checks. Permissions apply to the host directory
at component creation; a nonexistent host directory fails the load.

Network access requires both a protocol grant (`network.tcp` or `network.udp`)
and a matching entry in `network.addresses`. Entries contain an IP literal
and optional port; omitting the port explicitly grants all ports on that IP.
These grants cover binds and connections. `network.dns` separately enables
name resolution and does not authorize access to resolved addresses. All
three switches default to false, and the address list defaults to empty.
P2 calls are synchronous: fuel and epoch interruption bound guest execution,
not time spent in blocking P2 host I/O. Canonical async calls use the suspension
and cancellation path described above.

With providers enabled, the `wasi:` import namespace is provider-owned;
application callbacks cannot override it. Generated `ApplicationImports`
omits those interfaces while preserving required custom functions. Unsupported
interfaces still fail instantiation. Enabling WASI registers `wasi:http/types`
and `wasi:http/outgoing-handler`; HTTP requests require TCP plus the exact
destination grants above, and DNS permission when the authority is a hostname.
Perry's `perry-wasi` providers adapt the WASI 0.2.12 contracts from Wasmtime
48.0.5 and use Turnloop for native I/O. The selected host dependency graph and
workspace gate are Tokio-free. The required Turnloop repairs now live in pinned
repository sources selected by the root Cargo manifest; builds need no external
source override. Published alpha.10 still lacks required provider APIs; see
[provider provenance](../crates/perry-wasi/PROVENANCE.md). The selected provider
passes 40 host tests, 181 provider tests and 26/25 driver tests in development
and production panic-abort on Linux. Fresh matching compiler/runtime builds pass
all 28 native, nine command/library and 34 guest/composition acceptance phases,
including eight live moving-GC cases. Both ordinary and moving network runs
verify six UDP and three TCP payloads. See the
[selection receipt](validation/wasi-turnloop-selected-20261023.json).
The later [self-contained source receipt](validation/wasi-self-contained-turnloop-20261026.json)
records normal locked builds with 277 provider, 189 engine, 26/25 I/O and 22 WIT
tests, plus standalone Rust-host typed calls and direct composition without an
external source override.
Other platform and production-package qualification remain. Historical receipts
below predate this provider replacement.
The HTTP client uses bounded owned body channels, verified TLS and Turnloop's
HTTP/1.1 codec on the same context driver. Uploads progress before the response
future is polled; early final responses can reject unfinished uploads. Header
views retain their resource parents, request options are consumed at dispatch,
and body streams must be dropped before their body is finished. Body resources
can outlive their request/response handles. Finishing an incoming body drains to
actual trailers; dropping a client response body cancels its transport. DNS permission does not grant
any resolved IP. There is no automatic redirect, decompression or HTTP/2 support.

The [HTTP client receipt](validation/wasi-http-client-20261023.json) records Linux
provider/host tests, a real WASM HTTP round trip through both provider linkers,
and the production Host facade's opt-in, TCP, DNS and exact-address denials.
That receipt covers the client; incoming-handler qualification is recorded below.
Perry-generated TypeScript HTTP guests under moving GC have the separate
qualification below. Other native platforms and final shipping packages remain open.
The experimental `informational-outbound-responses` interface is disabled.
Native Perry WASI 0.3 integration remains incomplete. The provider crate
has optional `p3` clocks/random, CLI, filesystem, socket and HTTP providers at 0.3.0, now
linked by the optional component engine. See [provider provenance](../crates/perry-wasi/PROVENANCE.md)
and the [Linux filesystem qualification](validation/wasi-p3-filesystem-20261023.json)
and [socket qualification](validation/wasi-p3-sockets-20261023.json), plus
[HTTP qualification](validation/wasi-p3-http-20261023.json). HTTP supports
canonical body streams, trailers and transmission futures, async client calls
and Rust handler embedding over Turnloop HTTP/1.1. Native application
future/stream readers and JavaScript producer factories are implemented.
Native TypeScript bindings expose
`Component.handleHttp` for the pinned handler; Rust embeddings can invoke it
through `Host::handle_http_async`.

The Rust embedding can await `Host::handle_http_async` for the exact
`wasi:http/handler@0.3.0` interface. It takes an accepted native `server::Request`
and its `server::Completion`; it drives the same Store through body transmission
and post-return task exit. The outer result reports validation or guest traps,
and the inner result reports transport completion. Successful exchanges and
explicit guest HTTP errors preserve the instance. Traps, cancelled handling and
transport failures retire it. Receiving the request supplies no outbound grants.
The selected manifest must expose the handler's resource identities and result
contract; this typed Rust entry point does not enable generic application
`future<T>` or `stream<T>` bindings. The TypeScript HTTP API supports both
P2 incoming handlers and the exact P3 0.3.0 handler.

Generated native bindings select `wasi:http/handler@0.3.0` explicitly for
`Component.handleHttp(server)`. Its public Promise remains rooted across
suspended imports and body I/O, retains its captured ALS context and settles
only after transport completion and task exit. The owning agent supplies fresh
primary and cleanup frames on every resumed poll. Explicit HTTP errors preserve
the instance for reuse. Closing the listener cancels even a handler waiting on
an unresolved JavaScript import; disposing the component also cancels handling.
A late trap rejects the Promise and retires the Store, but cannot retract bytes
already flushed to the peer, so that response may be truncated.

Supported P3 handlers can now overlap on one component. A response's public
Promise waits for its own transport flush and validated task exit while another
request remains suspended. Async imports, Promise reactions and cleanup retain
their submitting request's ALS context. Returning the function result is not
task exit. A private callback wrapper observes canonical `EXIT`; the driver
acknowledges it only after the VM poll succeeds. Traps during exit validation
still retire the Store. Completed roots are removed individually, with native
callback leases retaining context through their own cleanup.

This profile currently covers canonical lifts defined in the component's root,
including interface instances assembled from those functions. It preserves
original indices, canonical options and nominal resource identities. Imported
and nested implementations use the serialized fallback; stackful async,
component threading and general task-exit observation are not enabled. Private
wrapper instances count toward the configured instance limit.

For Rust embeddings, construct with `Host::new_for_driver`, call `into_driver`,
then submit owned native exchanges with `DriverClient::handle_http_async`.
`DriverHttpCall` preserves the existing outer validation/trap result and inner
transport result. Unsupported handler layouts reject this explicit driver API;
`Host::handle_http_async` remains available for serialized handling. ABI 12 adds
`perry_component_driver_http_profile` and `perry_component_driver_http_submit`:
profiling runs before instantiation, submission runs no guest code or callbacks,
and ordinary driver polling publishes the invocation's result. Requests and
completions cross this boundary as native owners, never application JSON.

The [concurrent HTTP receipt](validation/wasi-concurrent-http-20261026.json)
records 189 engine and 77 component runtime tests in development and production,
plus 19 production HTTP phases over plain/TLS and ordinary/moving-GC execution.
Its 52 exchanges include independent completion with a withheld upload, original
error identity and nested-handler fallback. Moving runs copied 5,502 objects
and protected 172 retired page sets. The identical concurrency fixture fails
against the preserved ABI 11 runtime with a pending-call rejection. Matching
ABI 12 archives also pass producer, reader, persistent, async and command
regressions. No Tokio dependency is introduced.

The [TypeScript P3 HTTP receipt](validation/wasi-p3-http-typescript-20261025.json)
records 75 native host tests in development and production panic-abort, 40 runtime
component tests, 16 WIT tests and strict TypeScript checking. Actual compiled
TypeScript hosts exercise plain HTTP and verified TLS in ordinary and moving-GC
modes, including 3 MiB binary bodies and trailers, two suspended imports per
successful request, explicit error reuse, original rejection identity, traps and
cancellation. That receipt covers ABI 8; the current boundary is ABI 12. Rebuild the compiler, runtime and optional
engine together. Matching archives also pass the full 28 library, nine command
and eight HTTP P2 regression phases. Other platforms, released Turnloop
dependencies and final distribution packages remain open.

The [P3 HTTP embedding receipt](validation/wasi-p3-http-host-20261025.json)
records live Linux requests with empty, 65,537-byte and 3 MiB bodies, trailers,
Store reuse, typed errors, preflight checks, cancellation, disconnects, fuel
exhaustion and traps after a handler publishes its response. One-shot P3 commands
also wait for task exit after publishing their result; a suspended late trap
must throw instead of becoming an early status 0.

The Rust embedding API now provides an HTTP/1.1 origin server through
`perry_wasi::http::server::Server` and canonical handler invocation through
`perry_component_host::Host::handle_http`. The host explicitly supplies a bind
address, connection limit, deadlines and optional TLS credentials. This is
host authority: a component can handle an incoming request with no outbound
TCP, DNS or IP grants. It cannot acquire a listener through these WIT resources.
There is no ambient certificate or environment lookup.

One owned Turnloop task per exchange drives both bounded request and response
channels. The guest runs on the caller's Store; no Store or guest callback
enters the native driver. Dropping an incoming server request body switches to
discarding input and does not cancel its response. The separate completion owner
must remain alive through handling; dropping it cancels the exchange. Dropping
the server cancels its accepts and exchanges and joins its driver. A bounded
half-close/drain protects an early response from unread-upload resets.

The embedding receives an owned request and response sender, calls
`handle_http` with its selected incoming-handler interface, and awaits the
transport completion even if the handler failed, so the transport can flush
its error response. A trap, fuel exhaustion or return without setting the
response retires that component instance. Explicit `response-outparam.set(err)`
is a valid handler return, produces HTTP 500, and remains an error in transport
completion. Missing exports and wrong canonical signatures fail before native
resources are inserted. A completion means transport processing finished;
it does not prove that the peer consumed the bytes.

The [HTTP server receipt](validation/wasi-http-server-20261023.json) records
Linux development and production panic-abort qualification: large streamed
requests/responses and trailers at admission limit one, early rejection,
HEAD and 100-continue, TLS and handshake failures, invalid framing, timeouts,
cancellation and shutdown. A real WASM incoming-handler reads and writes body
streams through the production Rust Host facade, handles repeated requests
with no outbound grants, and exercises explicit errors, traps, missing responses
and fuel exhaustion. Canonical preflight rejects the wrong signature.
This is Rust embedding and canonical-WASM qualification. The TypeScript host
API and Perry-generated HTTP fixtures with moving-GC qualification are described
below. The server currently accepts HTTP/1.1 origin-form requests
(and OPTIONS `*`), one request per connection; it does not implement HTTP/1.0,
connection reuse, HTTP/2, CONNECT tunnels or upgrades.

ABI 6 adds a nonblocking HTTP bridge for the optional engine: owned server and
operation handles, native wake registration, caller-thread handler invocation,
and separate transport completion. Waiting for an accept or completion never
executes guest code. Closing a server invalidates pending and already-ready
operations; native callbacks carry an agent identifier, never a Perry heap
address. Handler and retirement-cleanup callbacks use separate exception contexts
so a cleanup exception cannot replace a primary failure. The
[HTTP ABI receipt](validation/wasi-http-ffi-20261023.json) qualifies this boundary
on Linux in development and production panic-abort. Matching ABI 6
compiler/runtime qualification is recorded below; final packages remain open.
Earlier combined acceptance receipts used ABI 5.

The native `perry/component` API provides `createComponentHttpServer(options)`,
`handleComponentHttpRequest(server, component, interfaceName): Promise<void>`,
and `closeComponentHttpServer(server)`. Generated host bindings for a P2
incoming-handler world expose `component.handleHttp(server)`, supplying the
exact exported interface version. For example, after loading such a component:

```ts
import { createComponentHttpServer, closeComponentHttpServer } from "perry/component";
const server = createComponentHttpServer({ address: "127.0.0.1:8080" });
try {
  await component.handleHttp(server); // accept and serve one request
} finally {
  closeComponentHttpServer(server);
}
```

`server.address` is read-only and includes the selected port when binding port
zero. Options accept a connection limit, transport timeouts in milliseconds,
and explicit TLS certificate/key PEM strings. Each call accepts one request;
the Promise resolves after response transport completion. Closing the server
cancels outstanding accepts and exchanges. Disposing a component cancels its
uninvoked accepts. A handler trap retires the component; an explicit error
response rejects the Promise while leaving the component reusable.

Pending operations retain their Promise, component, server and captured async
context through traced ordinary objects. Native wakers only notify the owning
agent; the agent pump invokes guest/JS callbacks. The pump prevents recursive
Store entry and preserves the first handler exception through retirement and
transport cleanup. Accept and response completion are asynchronous; the P2
handler invocation itself is synchronous on the owning JS thread. Transport
timeouts bound body waits; instruction fuel does not bound time in host I/O.
`scripts/component_http_acceptance.py` exercises this API
with a canonical WASM fixture over plain TCP and TLS, including moving GC;
its execution is a separate qualification from the native engine receipts.
The [HTTP runtime receipt](validation/wasi-http-runtime-20261023.json) records
60 unit tests and 24 requests in the final ordinary/instrumented acceptance.
Both instrumented transport runs copied 823 objects and reported quarantine
and evacuation verification witnesses. Ordinary and instrumented archives use
the same compiler/engine package and feature union. This qualifies Linux
development native hosting of the canonical fixture.

The [authored HTTP guest receipt](validation/wasi-http-guest-20261023.json)
qualifies a Perry-compiled component importing outgoing-handler and exporting
incoming-handler at WASI 0.2.12. Eight cases cross plain HTTP/verified incoming
TLS with ordinary, guest-moving, host-moving and combined moving GC. They pass
56 incoming requests, 32 outgoing echoes and 88 typed capability denials, with
exact binary round trips through 3 MiB + 137 byte bodies. They also cover async
context, explicit-error reuse, trap/missing-response/fuel retirement and pending
server/component cancellation. Both heaps show actual copies and evacuation
verification; native runs additionally protect retired from-space. No WASM
quarantine claim is made. Authored outgoing requests use plain HTTP in this
matrix; TLS transport has separate native qualification.

Canonical byte-list lifting now copies into a freshly rooted Uint8Array in one
bulk operation. A regression probe passes a 3 MiB argument under the original
one-billion-instruction budget and reaches the typed HTTP denial; the previous
per-element setter exhausted fuel before HTTP. The 35 component runtime unit
tests include storage independence and survival through moving collection.
The final compiler reproduces the tested guest archive byte for byte; native
ordinary/instrumented artifacts use the same package and feature union.
The [production HTTP receipt](validation/wasi-http-production-20261023.json)
repeats all eight transport/GC cases with matching `prod` compiler, runtime,
stdlib and engine artifacts, including instrumented archives. All 12 phases
pass, and the production compiler reproduces the tested WASI guest archive
byte for byte. This qualifies the frozen HTTP source snapshot; the later P3
provider additions have separate receipts. Other platforms and final matching
distribution packages remain open.

Flag declarations are limited to 1–32 labels, matching the
[Component Model binary format](https://github.com/WebAssembly/component-model/blob/main/design/mvp/Binary.md#type-definitions).

The value mapping uses BigInt for `s64/u64`, Uint8Array for `list<u8>`, and
tagged objects for options, results, and variants. In particular,
`some(none)` stays distinct from `none`. Resource wrappers retain private
ownership tokens; application objects cannot forge a token by adding public
properties. Native async functions use the Promise mapping above. Native future
and stream results use owned readable handles, with component-bound native
producer factories. Async guest adapters remain unsupported.

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

The internal C frame API also provides `perry_component_frame_promise_poll`.
It returns pending, a fresh rooted fulfillment slot, or a frame error holding
the original rejection. Polling marks an actual Promise handled without running
jobs, invoking thenables or waiting for I/O. This is an async adapter foundation;
the public generator still rejects async guest functions and future/stream
values until task scheduling and canonical suspension are implemented.
The [Promise observation receipt](validation/wasi-guest-promise-observation-20261026.json)
records Linux development and production panic-abort tests for real moving
collection, independent result ownership, rejection identity and invalid inputs.

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
below. That receipt excludes blocking-I/O cancellation and HTTP; the later
HTTP client qualification is recorded above. Command execution is qualified
separately above.

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

- Connect the TypeScript server API and qualify Perry-generated HTTP guests
  under moving GC, remaining capability operations and blocking-I/O cancellation.
- Qualify supported host platforms. [PR #12985](https://github.com/PerryTS/perry/pull/12985)
  added the component acceptance suite to required execution checks; integration
  with that workflow exercises these extended resource fixtures.


Completing P3 async support remains a subsequent milestone.

## Native host platform qualification

Source `17318d3ad30db9d43fc09193e3efb1a3f1729381` passes the expanded native
host profile on Linux x86_64 and macOS ARM64. Each platform executes all 28
library phases, nine command phases, 40 component-engine unit tests and four
loop-interrupt regression tests. The same run passes 57 P1 cases and the
platform-specific P1 host unit suite.

Independent verification checks every phase, exact normal/moving TCP and UDP
payloads, and seven moving-GC workloads across P1/P2 against their retained
logs. The [platform receipt](validation/wasi-native-host-platforms-20261021.json)
records downloaded artifact digests, retained file hashes and CI-recorded build
artifact identities. Windows native hosting remains pending. This qualification
covers native hosting, not Perry-authored component output on macOS or final
production packages. HTTP on these host platforms, blocking-I/O cancellation
and P3 remain open.

## Continuous execution checks

`scripts/component_check.sh` builds matching native tools, ordinary and
instrumented runtime archives, and the WASI runtime, then runs native, command,
guest and persistent future/stream acceptance. Configure wasi-sdk first, as for
the standalone WASI checks. Fixture
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
