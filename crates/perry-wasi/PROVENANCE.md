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
| `src/p2/{network,ip_name_lookup}.rs`, `src/p2/host/{network,instance_network}.rs` | Corresponding `wasmtime-wasi` files and `src/sockets/ip_name_lookup.rs`; address conversions from `src/sockets/mod.rs` |
| `src/sockets/tcp.rs`, `src/sockets/tcp/` | WASI TCP validation, defaults and option semantics from `wasmtime-wasi/src/sockets/{mod,tcp}.rs`; native ownership and I/O are implemented with Turnloop |
| `src/p2/host/{tcp,udp}.rs` | Synchronous forwarding adapted from corresponding `wasmtime-wasi` files; state and I/O use Turnloop |
| `src/p2/stdio.rs`, `src/p2/host/{env,exit}.rs` | `wasmtime-wasi` files with the same paths |
| `src/error.rs`, `src/clocks/datetime.rs` | `wasmtime-wasi/src/error.rs` and timestamp conversions from `src/clocks.rs` |
| `src/pipe.rs` | Memory and closed WASI stream implementations in `wasmtime-wasi/src/p2/pipe.rs` (without Tokio adapters) |

Native readiness now returns a fallible result: timer/driver failures trap
through WASI poll/block/ready instead of reporting successful readiness. The
poll list still coalesces duplicate resources while reporting every ready input
index. Resource parent/child ownership and stream error conversion are retained.
Synchronous bindings wait on native futures with `futures-lite`; I/O and timer
work is owned and driven by Turnloop, with no ambient Tokio runtime.

The current port includes WASI 0.2.12 I/O, clocks, random, CLI, capability
filesystem, DNS, TCP and UDP interfaces. It is not yet selected by
`perry-component-host`. HTTP and native WASI 0.3 remain to be ported;
platform qualification is still required. No missing interface
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
released repair and rerun platform qualification before selecting this provider
in the component host.


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
matrix alone does not qualify this layer. HTTP, provider selection in the
component host and native WASI 0.3 integration also remain incomplete. No shipping dependency
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
is Tokio-free while the workspace gate still requires removing the old provider.
