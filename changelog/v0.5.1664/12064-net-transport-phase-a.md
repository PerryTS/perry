`net.Socket` and `net.Server` are native payloads with a TransportCore, and
their completions route by link token through the payload cell. perry-ext-net
no longer keeps id-keyed socket or server tables (19 tables and 10 producers
in the native handle ledger, now 0 and 0), so a Socket that closes and is
dropped is collected and its WeakRefs clear, as in Node.

HTTP/1 server and client connections are `net.Socket` payloads. An HTTP
`upgrade` is a single store to the connection's route: the listener gets the
same Socket, with the bytes that arrived with the head as `head` and every
later byte as its own `data`. The upgrade listener's socket parameter is an
ordinary value; the compiler no longer tags it as a `ws` Client, which used to
register its `data` listener in a ws id table that never fired. A server with
an attached `WebSocketServer` still hands the listener a ws client handle,
which dispatches through ws's own handle methods.

The socket-open audit now requires one `SocketFields.opened` declaration and
live connect/accept transitions, and rejects duplicate Rust/JS flags and
id-keyed maps. Bun's duplicate `bunOpened` latch is removed: its existing
pending connect promise selects `connectError`. The workflow step is retained.

Registry lifetime census: current main has 249 maps (167 with removal paths),
phase A has 225 (146 with removal paths). All 24 disappearing identities are
net/HTTP/TLS tables; none are added. Parser-liveness floors are adjusted from
250/170 to 220/140 solely because those tables were deleted.

Review notes: main already violates the native handle ledger because
`buffer/header.rs` moved to `buffer/store/layout.rs`. The seven ws Client
codegen rows and their manifest entries are unused after removal of the HTTP
upgrade parameter hint; delete them in the immediate follow-up. Linux ext-ws
lib tests fail to compile on main (`js_shadow_frame_push` is absent with native
stack maps). Main also fails issue_9619's manual/callback-only path and
`nested_object_literal_ws_inbound`: ws handleUpgrade has no raw-socket path.

Round 2 selects net constructor families from immutable class captures in a
canonical constructor layout. Explicit receiver calls and dynamic super use
that fact even after function names and prototype constructor properties are
changed. Dynamic super shares one validated parent resolution with the existing
native-export dispatch; ordinary parents do not repeat the header or registry
lookup.

Native completions use callback-free payload windows, ending every borrow
before JS, close or reopen. HTTP batches parser and native I/O steps within a
proof. Opaque record reads and overwrites avoid allocating property-key strings;
inline and overflow stores preserve the runtime write barriers and moving-root
edges. Emitter arguments and async provider hooks share one dispatch scope and
short argument spans stay inline. Empty write acknowledgements skip callback
work, and plaintext allocates a JS Buffer only for a listener that can receive it.
Deferred events keep their required traced captures. No cache, table or latch is
added.

Round 2 validation: ext-net 22 (including 100k lifecycle), ext-http 208, moving
record stores and borrowed roots pass. Constructor mutation/heritage fixtures
pass after the shared-super fix. N2/N3/N4/N8 sabotage witnesses reject their
intended faults. The full gap subset has zero regressions: main 87 pass, 15
mismatch, one timeout, two lane-only; lane 90 pass, 15 mismatch, no timeout.
The final heritage change rechecks its affected fixtures. Script lint is
102/125 on main and 103/125 in lane, with no new failing gates. Changed-crate
Clippy adds no warning/file pairs; strict workspace Clippy inherits main's
perry-dispatch large_const_arrays failure. Lean/pruned dependency gates pass.

A/B is pinned to the required initial rebase base 678ab79517, with separate
Linux targets, CPUs 0–55, setarch -R, n=5 interleaved instructions:u/RSS. Main
advanced while this work ran. TSC +0.011%, Zod -0.006%, fastify +0.392%,
buffer +0.007% and worker +0.069% instructions are within the observed
within-arm spans. Hello has a reproducible +709-instruction startup offset
(+0.052%); exact symbol attribution is still unisolated.

Net echo is improved to +1.10% instructions and HTTP 10k to +4.88%, but the
requested flat-micro/RSS target is unmet. Perf profiles still show payload
projection, opaque-record access and rooting outside the batched completions.
THP-off RSS remains +964 KiB net and +2,604 KiB HTTP. Net's anonymous heap
is smaller; its remaining footprint is fixed resident code pages. HTTP's
request/response records each grow by 40 bytes, amplifying existing app
registry retention proportionally with request count. Follow-up: carry proven
capabilities through Agent/request/response operations, compact or reclaim
app records according to traced JS lifetimes without breaking req.socket or
delayed-generation checks, and isolate the cold native-constructor branch
from common closure dispatch. No new side table, cache or latch is proposed.

Real RSS controls: TSC's delta changes sign and worker ranges overlap with
THP off; fastify retains +2,324 KiB, chiefly resident executable mappings
(+2,404 KiB text at exit) plus +368 KiB anonymous memory. Buffer's anonymous
heap differs by only +4 KiB. Reduce duplicated inlined proof/dispatch bodies
in the follow-up and remeasure code residency; exit probes do not exactly
decompose peak RSS. Full-collection counts are hello/Zod/net/HTTP 0/0,
TSC/fastify 1/1, buffer 36/36, worker 42/41 (main/lane).
