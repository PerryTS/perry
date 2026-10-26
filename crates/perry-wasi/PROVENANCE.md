# Provider provenance and migration scope

## Current source selection

The root Cargo manifest selects repository copies of the exact `turnloop` and
`turnloop-http` alpha.9 Rust sources used for Linux qualification. The direct
`turnloop-io` alpha.9 registry sources are byte-identical to the qualified copy.
Builds therefore no longer require the external source overrides described in
the historical records below. See
[the pinned source record](../../third_party/turnloop/VENDORED.md) for registry
checksums, source hashes, licensing and the alpha.10 API comparison. The normal
workspace lockfile selects these copies; no Tokio or publish-age exception is
introduced. This is a committed local source repair, not a claim that an
unmodified published Turnloop version supplies every required API.
The [self-contained build receipt](../../docs/validation/wasi-self-contained-turnloop-20261026.json)
records the locked dependency graph, test binaries, source identity checks and
standalone Rust-host qualification for this selection.

Native P3 engine integration, generated native TypeScript async calls,
future/stream readers and producers, and supported concurrent HTTP layouts now
have Linux qualification in [the component record](../../docs/WASI_COMPONENTS.md).
Async Perry-authored guest adapters, broader host platforms and final packages
remain open. Later source selection does not retroactively requalify earlier
receipt snapshots.

## Original provider port and qualification history

This crate adapts WASI contracts and native helpers from Bytecode Alliance
Wasmtime **48.0.5**, distributed under **Apache-2.0 WITH LLVM-exception**. The
full license is in `LICENSE`. The source packages are `wasmtime-wasi` and
`wasmtime-wasi-io`, using their 48.0.5 sources. The selected host no longer
depends on `wasmtime-wasi`; it retains the Tokio-free `wasmtime-wasi-io`
resource contracts.

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
| `src/p3/filesystem.rs`, `src/p3/filesystem/host.rs` | Conversions and descriptor forwarding adapted from `wasmtime-wasi/src/p3/filesystem/{mod,host}.rs`; stream/directory ownership uses Turnloop, and signed-time conversion is corrected |
| `src/p3/sockets/conv.rs`, `src/p3/sockets/{tcp,udp,dns}.rs` | Address conversions and interface forwarding adapted from `wasmtime-wasi/src/p3/sockets/{tcp,udp}.rs` and `src/sockets/{mod,ip_name_lookup}.rs`; socket ownership, cancellation, streams and native requests use Turnloop |
| `src/p3/http/{types,options,conv}.rs` | Interface forwarding, request options and conversions adapted from `wasmtime-wasi-http/src/p3/{types,options,mod}.rs`; body ownership, streaming, cancellation and transport use Turnloop |
| `src/p2/filesystem.rs`, `src/p2/host/filesystem*` | `wasmtime-wasi` files with the same paths |
| `src/p2/{network,ip_name_lookup}.rs`, `src/p2/host/{network,instance_network}.rs` | Corresponding `wasmtime-wasi` files and `src/sockets/ip_name_lookup.rs`; address conversions from `src/sockets/mod.rs` |
| `src/sockets/tcp.rs`, `src/sockets/tcp/` | WASI TCP validation, defaults and option semantics from `wasmtime-wasi/src/sockets/{mod,tcp}.rs`; native ownership and I/O are implemented with Turnloop |
| `src/p2/host/{tcp,udp}.rs` | Synchronous forwarding adapted from corresponding `wasmtime-wasi` files; state and I/O use Turnloop |
| `src/p2/stdio.rs`, `src/p2/host/{env,exit}.rs` | `wasmtime-wasi` files with the same paths |
| `src/http/fields.rs`, `src/http/error.rs` | `wasmtime-wasi-http/src/{field_map,error}.rs`; no upstream HTTP transport dependency |
| `src/error.rs`, `src/clocks/datetime.rs` | `wasmtime-wasi/src/error.rs` and timestamp conversions from `src/clocks.rs` |
| `src/pipe.rs` | Memory and closed WASI stream implementations in `wasmtime-wasi/src/p2/pipe.rs` (without Tokio adapters) |

Native readiness now returns a fallible result: timer/driver failures trap
through WASI poll/block/ready instead of reporting successful readiness. The
poll list still coalesces duplicate resources while reporting every ready input
index. Resource parent/child ownership and stream error conversion are retained.
Synchronous bindings wait on native futures with `futures-lite`; I/O and timer
work is owned and driven by Turnloop, with no ambient Tokio runtime.

The current port includes WASI 0.2.12 I/O, clocks, random, CLI, capability
filesystem, DNS, TCP, UDP and HTTP interfaces. It is selected by
`perry-component-host`. Optional WASI 0.3 providers are described below;
production engine integration remains open;
other platform and production-package qualification are still required. No missing
interface is implemented as a success stub. The unchanged workspace Tokio
gate passes: no manifest edges, lockfile packages or Rust source references.

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

The provider world imports the network capability, instance-network and
ip-name-lookup interfaces. DNS permission defaults to denied, including literal
addresses. Domain names are validated and converted to IDNA before an owned
Turnloop resolver request. Only owned names and addresses cross the driver
boundary. Pollables retain their resolver resource; dropping a pending stream
cancels delivery and queued work. Running native resolution may finish with its
owned input. Native lookup errors currently map to `name-unresolvable`, matching
the upstream Rust resolver limitation; distinguishing temporary/permanent native
resolver failures requires a richer Turnloop result. Driver loss and admission
failure trap. Literal and resolved IPv4-mapped IPv6 addresses are canonicalized
as required by `resolve-next-address`. TCP and UDP interfaces are registered
as described below.

Native Unix stdio integration requires the shared-ownership repair in
[Turnloop #119](https://github.com/PerryTS/turnloop/pull/119). Published alpha.9
restores flags per duplicate: closing one overlapping handle can make another
live handle blocking, then leave the original host handle nonblocking after the
last close. The repair also preserves terminal settings across overlapping
adoptions. A validation-only override to Turnloop commit
`5727b5c8a6d69fb486d8700d9ec3f019c4d2a63f` passes Perry's native overlap probe,
142 provider tests in both development and production panic-abort profiles,
and 12/11 driver tests respectively. This is not a shipping dependency update:
Perry's committed registry dependency and source policy are unchanged. Adopt the
released repair and rerun platform qualification before shipping this provider
selection.


The native TCP state machine now supports explicit and implicit binding,
bound-socket connection, listening, independent accepted connections, socket
options, bounded owned reads/writes, and independent receive/send shutdown. IPv6 sockets
are IPv6-only. TCP and per-address capabilities default to denied; callbacks
run on the calling agent, including accepted-peer checks. Pending accepts are
cancellable, denied peers are closed, and driver failures trap separately from
native socket errors. The adapter rejects concurrent reads or writes so a
caller cannot consume another operation's completion. Cancelling a partial
write disables further writes on that socket; a cancelled read retains native
pending/buffered data for the next read.

Linux development and production panic-abort validation passes all 169 provider
tests (including eleven native TCP and nine TCP WIT cases), plus 26/25 driver tests, doctests,
all-target Clippy and the stdio overlap probe. Validation uses the immutable
Turnloop revision `4340f396a2042722d3897f5c525295e1ef8032f7`, containing
[bound TCP #120](https://github.com/PerryTS/turnloop/pull/120) and
[independent controls #121](https://github.com/PerryTS/turnloop/pull/121), and
[receive shutdown #122](https://github.com/PerryTS/turnloop/pull/122).
The normal dependency graph for these provider crates is Tokio-free. This is
an external validation override, not a committed dependency-policy exception.

Receive shutdown follows explicit read-request cancellation and acknowledgement,
then shuts down the native receive direction and discards Turnloop's retained
input. It is idempotent, returns EOF on later reads, and preserves outgoing
bytes and FIN. Attempting it while another read request is active is a typed
concurrency conflict. Pending accepts own their listener reference and may
outlive the caller's wrapper; accepted connections inherit the current options
at completion, including changes made while acceptance was pending.

TCP and TCP-create-socket interfaces are registered in both synchronous and
asynchronous linkers. The adapter implements start/finish states, same-context
network capability identity, readiness and typed errors. Socket pollables and
input/output streams are child resources; accepted sockets have independent
lifetimes. Stream reads/writes own bounded 64 KiB buffers, enforce output
permits, and keep driver failures as repeatable traps. Receive shutdown cancels
and acknowledges a pending read; send shutdown drains a pending write before
FIN. Native shutdown progresses without further guest output polling.

Each native resource has two cleanup task slots, separate from the ordinary
work budget, so a saturated task budget cannot prevent either half from closing.
Cleanup retains the native resource until completion or acknowledged cancellation;
cloned handles share the two-slot limit. The executor reserves three times the
ordinary task limit: ordinary tasks plus two cleanup tasks per live resource.
Driver teardown cancels and joins all three classes of work.

The TCP WIT tests include IPv4/IPv6, real 0.2.0 components against both linkers,
capability identity and denial, failed connection closure, stream/pollable child
ownership, accepted-socket options, EOF and half-close behavior, repeatable driver
traps, and cleanup at a one-task limit. A bounded-window peer verifies all 64 KiB
arrive before FIN after explicit shutdown or output drop, without output polling.
Before the cleanup repair, regression tests reproduced a receive-shutdown
capacity trap and a stream-drop FIN timeout; both now pass. The provider's normal
dependency graph remains Tokio-free.

Native provider macOS/Windows qualification remains; the upstream Turnloop
matrix alone does not qualify this layer. HTTP and native WASI 0.3 integration
also remain incomplete. No shipping dependency
policy exception or unsupported-platform qualification is implied by these tests.


UDP owners preserve the bound socket and options through stream reassociation.
Address checks run on the caller; only native socket ownership, addresses and
owned datagram bytes cross the driver boundary. Binding is exclusive and IPv6
sockets are IPv6-only. Connected streams use exact-peer receive filtering and
explicit send destinations; association does not call native UDP connect or
change a wildcard local address. Old streams must be dropped before association
changes, and their pending receive/send cleanup is retained by the socket until
completion. Filtering is bounded per native poll so rejected peers cannot
monopolize the driver. Stream receive returns at most one full datagram per call;
check-send permits one datagram, and an absent/exceeded permit traps. Empty lists
and real zero-length datagrams are distinct. Native errors remain typed;
submission/readiness failures remain repeatable traps.

The executor stages 64 KiB per operation to preserve ordinary full-size UDP
packets. Operation slots are bounded by four times the configured task limit,
with a minimum of sixteen, including spare capacity for cancellation and cleanup.
This replaces the fixed 1,024 slots of 16 KiB; both arithmetic bounds are checked.
The independent task/resource/cleanup admission limits remain enforced.

The final Linux qualification passes 181 provider tests in both development and
production panic-abort, plus 26/25 driver tests, two doctests (one inherited
illustrative doctest remains ignored), all-target Clippy and the stdio overlap
probe. It includes five native UDP and seven UDP WIT tests, alongside the TCP
coverage above. These exercise real IPv4/IPv6 packets up to 65,507 bytes, zero
payloads, sender addresses, exact-peer filtering, unconnected destinations,
reassociation, cancellation, exclusive bind, capabilities, retained options,
permits, table ownership and saturated cleanup. A real 0.2.0 component sends an
empty datagram and validates the reply and sender through both linkers.

This uses an external validation-only patch to immutable Turnloop revision
`b63a4ee3d3ad73d2df61b35202ed140f05513053`, adding
[datagram adapter #123](https://github.com/PerryTS/turnloop/pull/123) atop the
previous repairs. Its own complete CI matrix is green, including Windows and
WASI 0.3, but that does not replace Perry provider platform validation. The
committed registry pin/source policy is unchanged; the provider dependency graph
and the complete workspace Tokio gate are now clean.

The component host now uses these providers for optional WASI imports and
command execution. It preserves explicit capability configuration and validates
command exports before initializers run. Linux qualification of this selection
passes all 40 host tests, 181 provider tests and 26/25 driver tests in development
and production panic-abort, plus doctests, all-target Clippy and stdio overlap.
Fresh matching Linux compiler/runtime builds also pass 28 native component,
nine command/library and 34 guest/composition phases. This includes independent
Rust hosting, direct Rust/Perry composition, Perry hosting Perry, explicit
capabilities and resource cleanup. Eight moving-GC cases require actual copied
objects and successful evacuation verification; the seven native cases also
observe protected retired memory. The WASM guest cannot use native memory
protection and is recorded separately. Ordinary and moving network peers each
verify six UDP and three TCP payloads.

The [selection receipt](../../docs/validation/wasi-turnloop-selected-20261023.json)
records exact source, patch, compiler, archives, executables and log hashes.
Windows/macOS provider and production-package qualification remain, as do
HTTP, native WASI 0.3 and released Turnloop adoption.


HTTP integration now has bounded body-streaming primitives and header resources,
using the `wasi:http@0.2.12` contracts from Wasmtime 48.0.5 revision
`563544c6296610b1b3fbbe2e4643f9999899ec66`. These do not yet register HTTP imports:
HTTP/TLS transport, capability wiring and the canonical resource adapters remain
required before claiming `wasi:http` support.

Header snapshots use copy-on-write ownership. Mutations validate names, values,
forbidden transport headers and configured memory limits; failed bulk updates or
header-capacity reservations leave both values and accounting unchanged.
Content-Length accepts decimal digits within u64, without a sign or whitespace.

Body channels retain one owned chunk of at most 64 KiB. The outgoing body's
completion owner is separate from its output stream: dropping the stream does
not finish the body, and dropping an unfinished body reports a protocol error.
Output permits enforce backpressure; excess Content-Length bytes are rejected
before delivery. Incoming streams permit partial reads, retain typed failures,
and distinguish EOF from failure. Finishing an incoming body discards unread
bytes while the producer drains to trailers, retaining length checks; trailer
results can be retrieved only once. Parent completion requires child-stream
release. Queue capacity indicates transfer to the HTTP consumer, not peer receipt.

Twenty focused tests cover these semantics, including actual Turnloop driver
production/consumption of owned bytes, timer progress beside backpressure,
cancellation acknowledgement and context teardown. Linux validation passes all
201 provider, 40 host and 26/25 driver tests in development/production panic-abort,
plus doctests, all-target Clippy, stdio overlap and the unchanged Tokio gate.
This body-layer coverage does not establish native HTTP interoperability or TLS.

## Native HTTP transport qualification

`http::transport` implements owned HTTP/1.1 client exchanges on the existing
component driver. It uses Turnloop's HTTP codec and TLS adapter. The TLS adapter
receives the driver's existing `ExecutorHandle`; `turnloop-io` reexports those
same executor types and does not construct another executor in this path. The
Node Loop's sans-I/O restriction does not prohibit this component-driver use.

Requests start at submission, before the guest polls its response. Upload and
response headers progress concurrently; an early final response stops further
guest upload while its response body remains readable. A single driver task owns
the connection, parser, TLS state and bounded body channels. Dropping the pending
response or incoming body cancels that task. Finishing the body transfers its
lifetime to the trailer future, so unread data can drain to actual trailers.
Context shutdown cancels the native task and wakes the remaining guest streams.

HTTP permissions are explicit owned data, default-denied. DNS permission never
grants a destination: every resolved address is checked against exact IP and
optional port grants before connection. No policy callback, guest value or Store
crosses the thread boundary. TLS verifies trust and authority, supports explicit
additional CA data, and advertises only HTTP/1.1. It reads no ambient CA file or
environment override. Each exchange owns one connection; there is no connection
pool, automatic redirect, automatic decompression, HTTP/2 or upgrade transport.

Connect/DNS/TLS, response-header and between-byte deadlines are bounded by the
same driver. Between-byte deadlines restart on each network read and exclude
waiting for guest body consumption. Invalid methods, schemes, authority ports
and overflowing deadlines fail with typed errors. Header/framing storage and
body transfers are bounded; request and response trailers use the HTTP codec.

Linux validation at immutable snapshot `turnloop-provider-v54-src` passes 213
provider tests, 40 host tests and 26/25 driver tests in development/production
panic-abort profiles, plus doctests, all-target Clippy and stdio overlap. Twelve
new transport tests exercise multi-megabyte streaming without polling the
response first, early rejection of an unfinished upload, exact capability
denials after DNS, trusted/untrusted/wrong-authority TLS, first/between-byte
expiry and fragmented framing, short versus close-delimited bodies, HEAD,
trailer draining, cancellation/capacity recovery and driver teardown.
The receipt is `turnloop-http-transport-verification.json` in the integration
report's `turnloop-http-transport` evidence directory.

This remains transport-level qualification: canonical HTTP WIT registration,
network-option wiring in the component host, the incoming/server handler path,
and macOS/Windows provider qualification are still required. Validation uses an
external override for both `turnloop` and `turnloop-io` from immutable revision
`b63a4ee3d3ad73d2df61b35202ed140f05513053`. The latter's source matches published
alpha.9, but that newly enabled package has not cleared the repository's seven-day
publish-age window at validation time (published 2026-10-16 21:39:33 UTC). The
shipping registry lock still needs resolution after that window and adoption of
the released Turnloop repairs; no publish-age exception or shipping path override
has been introduced. The actual validation dependency graph and workspace Tokio
inventory remain Tokio-free. This is not a claim that the unchanged registry lock
can build these new dependency edges with `--locked`.

## Canonical HTTP client interfaces

The base provider world now imports `wasi:http/types` and `outgoing-handler`.
Both linkers register the same HTTP resources with their existing I/O resource
identities. The `http-provider` world also generates incoming-handler export
bindings for subsequent server integration. `http/p2/{handlers,error,options}.rs`
adapt Wasmtime 48.0.5 `wasi-http/src/p2/{types_impl,error}.rs` and
`request_options.rs`; `wit/deps/http.wit` comes from that crate's WIT dependencies.
No upstream HTTP execution, ambient runtime or worker implementation is copied.

All stable HTTP resource methods are implemented. Headers are immutable child
resources of their parent request/response; clones are independent mutable
snapshots. Body streams are children of bodies, while bodies may outlive the
request or response that produced them. Request options are consumed by the
outgoing handler, including typed failure paths. Futures preserve pending,
ready, and consumed states; readiness uses the same WASI poll implementation.
Stream errors retain native HTTP error payloads through `http-error-code`.
The upstream reverse conversion of response-header-size to request-header-size
is corrected and covered by a regression case. Response outparams deliver owned
native response bodies through a bounded one-shot channel; dropping an unset
outparam closes that channel as a failure, rather than producing a response.
The experimental informational-outbound-responses feature is disabled by the
linker's default options; its unavailable method cannot report fake success.

The production component Host copies its existing TCP, DNS and exact-address
network grants into the HTTP context as owned data. There is no additional
ambient permission and no guest callback on the native driver thread.

Immutable Linux snapshot `turnloop-provider-v59-src` passes 220 provider tests,
41 host tests and 26/25 driver tests in development/production panic-abort,
plus doctests, all-target Clippy, stdio overlap and the unchanged Tokio gate.
Seven additional WIT tests cover ownership, options consumption, typed errors,
request/response streaming, trailer completion, response outparams, and a real
WASM component through both linkers. The production Host facade additionally
executes that component with denied TCP, DNS, IP and port capabilities, confirms
that no denied request connects, and permits a hostname resolving to the exact
granted endpoint. The component and peer check actual HTTP payloads; every call
leaves an empty resource table. This includes semver linking of 0.2.0 guest
imports to the 0.2.12 provider contracts.

See `docs/validation/wasi-http-client-20261023.json` and the integration report's
`turnloop-http-wit` evidence directory. This qualifies the client at the engine
and canonical WASM boundary. Perry-generated TypeScript HTTP/GC fixtures,
incoming-handler execution/server transport, other platforms and final packages
remain required. The dependency release/registry-lock limitations documented in
the native transport section still apply.


## Incoming HTTP handler and server qualification

`http::server` adds an embedding-owned HTTP/1.1 origin listener. Its explicit
bind address and TLS configuration are host capabilities, independent of a
guest's outbound grants. The listener owns a `perry-component-io::Driver`; one
bounded task accepts and runs each exchange with concurrent request and response
pumps. TLS, sockets and codecs stay on that driver. The native channels carry
owned metadata/bytes; canonical lowering, the Wasmtime Store and guest execution
stay on the calling thread. No Tokio runtime or ambient server is introduced.

`perry-component-host::Host::handle_http` checks the selected canonical resource
signature before inserting native request/outparam handles, applies fuel/epoch
limits, and observes whether the guest called outparam.set. A successful error
response is distinct from not setting the outparam. Traps, fuel exhaustion and
unset responses retire the instance and release its resources. A valid error
response leaves the instance reusable. Ordinary successful calls do not drain
custom host resources retained by the guest.

The shared bounded decoder now handles both request and response messages.
Dropping a server input consumer discards remaining request bytes without
cancelling response output. An embedding-held completion owner controls exchange
cancellation independently of those resources. All failed accepts await native
teardown before returning, including flushing a protocol rejection. Server drop
cancels all operations and joins the driver. Final output is flushed before a
bounded Turnloop lingering half-close/drain, avoiding immediate close with unread
upload bytes. The response body wait is bounded too: an unfinished guest body
cannot keep a connection/task alive indefinitely. Errors after headers leave
framing incomplete rather than writing a false successful final chunk.

The origin server requires exactly one valid Host, rejects ambiguous framing
and forward-proxy/CONNECT targets, sends 100-continue when requested, and omits
HEAD/204/304 bodies on the wire. Host-owned framing adds connection: close.
No HTTP/1.0, connection reuse, HTTP/2 or upgrades are implemented. Explicit
TLS credentials permit HTTP/1.1 ALPN only. Timeouts and checked deadline overflow
cover handshake, request head, response head, native reads/writes and idle
response-body frames; no ambient file/environment certificate lookup occurs.

Snapshot v65 qualifies 231 provider, 44 host and 26/25 driver tests in
Linux development/production panic-abort. The HTTP subset includes 20 primitive,
12 client transport, seven canonical resource, 11 server transport and four
production Host facade tests. Server tests cover 3 MiB each direction with
trailers at task capacity one, early 413 before upload completion, dropped input,
missing/error responses, invalid Host/target/framing, request and guest-response
stalls, truncated input, unfinished output, explicit cancellation, server shutdown,
HEAD, 100-continue, TLS success, invalid TLS, handshake timeout and configuration
errors. The actual incoming WASM fixture verifies body I/O, repeated calls,
set(err), unset responses, traps and fuel exhaustion with no outbound grants.
Each valid canonical call leaves an empty provider table. Wrong signatures fail
before publication and leave the host reusable. Doctests, all-target Clippy,
stdio overlap and the Tokio-free normal host graph pass as well.

The [server receipt](../../docs/validation/wasi-http-server-20261023.json)
records exact tested inputs and evidence. Validation still uses the immutable
Turnloop repairs plus matching turnloop-io from the external patch described
above. The direct turnloop-io dependency supplies the same driver's half-close
adapter; it does not create another executor. Registry lock resolution and
released repairs remain shipping requirements. This does not qualify the public
TypeScript server API, Perry-generated HTTP/GC fixtures, native WASI 0.3,
macOS/Windows providers or final production/npm packages.


## Nonblocking optional-engine HTTP ABI

Component-host ABI 6 exposes owned HTTP listener and operation handles through
its existing versioned table. Explicit JSON options supply an IP bind address,
connection capacity, deadlines and optional certificate/key PEM. Native accept
and completion polling returns pending/ready/terminal status and registers a
process-lifetime wake callback carrying only an agent identifier. No Store,
JavaScript callback or managed address reaches the driver. Handler invocation
is a separate caller-thread operation. Request/response bytes still use the
bounded native channels, not JSON body serialization.

The operation is guarded against reentry before its state is borrowed, and the
component bridge is guarded before request ownership transfers. Closing the
listener invalidates even a request already ready for invocation. Dropping an
operation cancels its native future; late native wakes cannot reach freed
operation memory because their context contains only the agent identifier.
A retired component is reported separately from ordinary rejection. Primary
and retirement-cleanup callback contexts are separate, so an exception from a
cleanup destructor cannot overwrite the original call failure. Both contexts
must remain rooted and live for the entire caller-thread invocation.

Snapshot v70 passes 231 provider, 50 host and 26/25 driver tests in Linux
development/production panic-abort. Six new C ABI tests cover pending accept and
native wake delivery, real canonical request/response bytes, wrong-phase calls,
repeated-ready/terminal states, trap/unset/fuel retirement versus explicit error
responses, guarded reentry, wrong exports without retirement, closing pending
and ready operations, option/credential rejection, and separate cleanup context
with a failing retained-resource destructor. Doctests, all-target Clippy, stdio
overlap and the normal host graph remain passing and Tokio-free. The
[ABI receipt](../../docs/validation/wasi-http-ffi-20261023.json) records the
matching code and evidence. TypeScript Promise/pump integration and moving-GC
acceptance are still pending, along with fresh matching ABI 6 compiler/runtime
builds. This engine-level result does not supersede the outstanding platform,
WASI 0.3, released-dependency or final-package requirements.


## Optional WASI 0.3 clock and random providers

The `p3` foundation enables Wasmtime's component async ABI and registers
`wasi:clocks` and `wasi:random` at 0.3.0; CLI and filesystem additions are below. Their WIT files are copied unchanged
from `wasmtime-wasi` 48.0.5 `src/p3/wit/deps`;
[the WIT manifest](wit-p3/provenance.json) records their hashes and license.
No upstream async runtime/provider crate is linked.

Clock waits submit owned native timers to the context's existing Turnloop
driver. Store access ends before awaiting the request. Dropping a provider
future cancels its request; driver shutdown returns an error. System-clock
instants use signed floor seconds with nonnegative nanoseconds, including
fractional times before the epoch. Random byte reads honor the P3 short-read
contract and cap each allocation at 64 KiB and the configured context limit.

The canonical clock fixture uses async lowering, async lifting, task returns
and waitable callbacks. Tests interleave two instances in one Store, exercise
immediate completion and reuse, and retire a Store during a suspended call.
Wasmtime 48.0.5 retains guest tasks when their host call future is dropped: hard
cancellation must drop the Store. Provider-future cancellation alone does not
establish guest-task cancellation.

This is a Rust embedding foundation. The production Perry component engine
does not enable this feature yet; P3 HTTP, generated
TypeScript adapters, Promise/future/stream lifetimes and cross-language async
composition remain to be implemented and qualified.

The [foundation receipt](../../docs/validation/wasi-p3-foundation-20261023.json)
records 237 provider tests in both Linux development and production panic-abort
builds, all-target Clippy, doctests and the Tokio-free dependency graph.


## P3 CLI streams and completion futures

The optional `p3` feature also registers `wasi:cli@0.3.0`: arguments, environment,
initial directory, exit, terminal resources and stdin/stdout/stderr. The added
CLI WIT and its filesystem/socket dependencies are unchanged pinned inputs in
[the WIT manifest](wit-p3/provenance.json). Filesystem and socket declarations
resolve the upstream CLI worlds. Filesystem providers are now registered as
described below, along with socket providers. The terminal resource types are distinct from P2 resources.

CLI stream adapters reuse the same context's explicit native P2 stream views.
Readiness futures own native views, never Store borrows or guest memory. Reads
and writes use bounded 64 KiB chunks; writes reserve capacity before taking
bytes from the canonical source. Once bytes are accepted, stream completion
waits for native flushing even when cancellation is requested, since accepted
bytes cannot be put back. Dropping the Store remains the hard-cancellation
boundary. Recoverable errors complete the separate result future; native
driver failures trap. Closed stdin and dropping its readable end complete
successfully. Closed output reports a broken-pipe result.

The canonical fixture passes stdin's stream through Wasm into stdout and
returns both result futures. Tests cover exact-capacity and large binary
transfers, closed defaults, dropped input, output errors, delayed flushing and
late errors, plus a real inherited-stdio subprocess and preservation of the
original process handle after context disposal. This qualifies stream/future
transport in the Rust embedding; Perry's TypeScript async bindings and GC
bridge remain separate unfinished work.

The [CLI receipt](../../docs/validation/wasi-p3-cli-20261023.json) records
243 provider tests in both Linux development and production panic-abort builds,
all-target Clippy, doctests and a Tokio-free normal dependency graph. Cargo
checksum freshness and an explicit expected test count guard source selection.


The [production HTTP runtime receipt](../../docs/validation/wasi-http-production-20261023.json)
qualifies the frozen HTTP source with matching production compiler/runtime/stdlib
and engine builds. Its authored HTTP component passes all eight plain/TLS and
independent host/guest moving-GC cases. This is separate from the P3 provider
receipts and does not qualify final distribution packages or other platforms.


## P3 filesystem streams and async descriptors

The optional `p3` feature registers `wasi:filesystem@0.3.0` preopens and all
filesystem descriptor operations. Its bindings use the pinned WIT already in
[the WIT manifest](wit-p3/provenance.json). Async metadata and path operations
clone native capability handles before awaiting Turnloop work; Store borrows
and guest memory never enter the blocking pool. The existing path and access
checks apply, including read-only grants and rejection of absolute paths,
parent escapes and escaping symlinks. Resource-table failures and stopped
native drivers trap; filesystem errors retain their typed P3 result.

CLI and filesystem byte streams share owned native adapters, with separate
error mappings. Reads and writes are bounded to 64 KiB and output completion
waits for native writes to finish. Directory streams request at most 64 native
entries per batch, including metadata lookup, rather than collecting an entire
directory. Dropping a stream cancels queued work/delivery; an already running
native syscall can finish with its owned inputs. A closed readable stream
completes its separate result future successfully. This implements the pinned
stream-plus-result-future ABI; it does not claim an additional error-context
payload on the stream itself.

Signed timestamps use floor seconds and nonnegative nanoseconds in both
directions, including fractional pre-epoch values. Unix FIFOs and sockets
retain their P3 descriptor types; P2's existing unknown mapping is unchanged.

Acceptance coverage includes multi-megabyte binary transfers, offsets, append,
empty files, chunk boundaries, EOF, resource deletion while streams retain
native handles, directory batching and invalid UTF-8 names, typed denials,
metadata/path operations, signed file timestamps and stopped-driver errors.
Two canonical Wasm fixtures transfer filesystem streams/result futures and
perform async descriptor calls, releasing guest borrows before returning.
These are Rust embedding tests. Production Perry P3 integration, TypeScript
async/GC bindings, other platforms and final packages remain unqualified.

The [filesystem receipt](../../docs/validation/wasi-p3-filesystem-20261023.json)
records 252 provider tests in both Linux development and production panic-abort
builds, all-target Clippy, doctests and the Tokio-free dependency graph. Source
and binary hashes, checksum freshness and all nine added test names were verified.

## P3 TCP, UDP and name resolution

The optional `p3` feature registers `wasi:sockets@0.3.0` TCP, UDP and DNS using
the unchanged pinned WIT. Network access remains denied by default. TCP/UDP
grants, DNS grants and address-policy callbacks apply independently; callbacks
run on the calling thread. Native I/O uses the context's Turnloop driver.
No Store borrow, canonical memory or guest callback crosses an async I/O wait.

TCP supports explicit and implicit binding, asynchronous connect, a demand-driven
accept stream and one send/receive stream pair with typed completion futures.
Streams retain their native connection after the socket resource is dropped.
Writes use bounded owned chunks, and send completion waits for the native FIN
after accepted writes. Dropping the receive stream shuts down that direction.
Socket options remain usable while connecting through a duplicate control
handle to the same OS socket. Successful connection replaces this view; failed
or cancelled connection drops both owners. The bound port is then released,
even if the guest resource remains alive. Other connection attempts and state
transitions remain invalid while a connect is pending.

UDP independently serializes sends and receives, retains cleanup acknowledgements
after cancellation, preserves empty datagrams, and applies reassociation to
an already pending receive. Destination policy runs before implicit binding.
DNS resolves owned names and handles literals only with an explicit grant.
Native resolver failures currently map to `name-unresolvable`, matching the
pinned upstream Rust resolver limitation. Driver failures trap. Broken-pipe
I/O retains its P3 `connection-broken` code without changing the P2 mapping.

Three canonical Wasm fixtures exercise TCP byte streams and result futures,
async connect suspension/callbacks, and UDP async send/receive with reuse.
Additional tests cover multi-megabyte binary TCP, half-close, accepted socket
lifetime, option inheritance and updates during connect, IPv4/IPv6 UDP,
maximum IPv4 datagrams, cancellation, concurrent calls, reassociation and
denials. Every socket test checks that native resources and outstanding I/O
return to zero. The [socket receipt](../../docs/validation/wasi-p3-sockets-20261023.json)
records the final qualification. These are Linux Rust embedding results;
Perry's production P3 engine, TypeScript async/GC bridge, other platforms and
final distribution packages remain separate qualification work.


## P3 HTTP streams, futures and async handlers

The optional `p3` feature registers `wasi:http@0.3.0` types and async client
imports. It also exports generated async handler bindings for Rust embedders.
The unmodified WIT comes from `wasmtime-wasi-http 48.0.5`; its archive and WIT
hashes are recorded in `wit-p3/provenance.json`. Metadata forwarding is adapted
from that crate. No upstream HTTP execution, Tokio channel or transport code is
used.

Requests and responses own canonical contents streams, trailers futures and
transmission-result futures. Canonical callbacks touch the resource table only
on the Store thread. Native I/O receives owned bounded byte queues, with at most
one 64 KiB queued chunk per bridge. Stream backpressure and content-length errors
remain observable. Successful trailers wait until the returned body stream
closes; trailer errors interrupt transmission even while contents are pending.
Component-to-component forwarding uses the same bounded framing path.

Transmission futures retain the actual exchange outcome, including upload and
response completion; receiving response headers alone is not completion.
Dropping a pending native client operation or its incoming response cancels the
native exchange, even while another transmission observer retains its owner.
Guest task hard cancellation still requires dropping the Wasmtime Store; dropping
a host call future alone does not cancel every task inside that Store.

Header resources retain original spelling and ordering for copy-all and HTTP/1
serialization, with case-insensitive lookup and forbidden-header checks. The
ordered representation is charged against the field memory limit. Header and
request-option snapshots remain immutable while explicit clones are mutable.
Turnloop's HTTP/1 decoder requires the accompanying casing fix; its constructor
for HTTP/2 callers retains existing lowercase behavior.

The HTTP receipt in `docs/validation/wasi-p3-http-20261023.json` records exact
Linux development and production panic-abort qualification, all-target Clippy,
doctests and the Tokio-free dependency graph. Tests exercise real canonical
async client suspension and handler exports, 3 MiB + 137 byte bodies, forwarded
streams/futures, HTTP/TLS header and trailer round trips, explicit network/trust
configuration, denials, size errors, cancellation and empty Store/native I/O
state after disposal. The codec has separate field-casing and mixed-case Host
and forbidden-trailer regressions.

These are Rust embedding and handwritten canonical Wasm fixtures. Perry's
production P3 engine, generated TypeScript async/future/stream bindings and
Promise/GC bridge remain unfinished. Windows/macOS provider behavior, required
provider CI, released Turnloop dependencies and final packages remain open.
HTTP/1.1 transport restrictions described above still apply.
