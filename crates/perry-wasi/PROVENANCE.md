# Provider provenance and migration scope

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
filesystem, DNS, TCP and UDP interfaces. It is selected by
`perry-component-host`. HTTP and native WASI 0.3 remain to be ported;
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
