# turnloop P1 — `node:net` on turnloop handles

Branch `turnloop/p1-net`, based on `turnloop/p0-wait-driver`. Built and tested
on the shared Linux box (EPYC 9354P, nightly-2026-08-20, LLVM 22.1.8) against
the pinned gap oracle Node **26.5.1**; the runtime unit tests also ran on macOS
arm64. Nothing here was run on Windows.

## What moved, and what did not

| socket class | transport after P1 | why |
|---|---|---|
| TCP listener (`net.createServer().listen(port)`) | **turnloop** | cannot be TLS-upgraded |
| connection accepted by a listener | **turnloop** | ditto |
| local listener (`listen(path)`) — UDS, Windows named pipe | **turnloop** | ditto |
| local client (`net.connect({path})`) | **turnloop** | `upgradeToTLS` already refused IPC sockets |
| outbound TCP client (`net.connect(port, host)`) | tokio | see below |
| `tls.connect` / a socket after `upgradeToTLS` | tokio | TLS is P5 |
| any socket on a `worker_threads` agent | tokio | that agent has no loop until P3/P4 |

**Why outbound TCP clients stayed.** `socket.upgradeToTLS` hands a live
`TcpStream` to `tokio_rustls` mid-stream — Postgres' `SSLRequest` flow, and
`test-files/test_net_upgrade_tls.ts`. turnloop owns its descriptor and does not
expose it: `Detached` has no fd accessor in 0.1.0-alpha.2 **or** alpha.3. A
socket's transport is fixed at creation and whether a given client will be
upgraded is not knowable then, so moving the class would have broken the
upgrade. Two things unblock it, either one sufficient:

1. a way to take a connected transport back out of a loop — `Detached::as_fd()`
   / `into_fd()`, or a `Loop::detach` whose result the host can adopt; or
2. TLS on turnloop (P5), after which nothing needs the `TcpStream` at all.

## The tokio code this deleted

Per listening server: one `perry_ffi::spawn_async` accept loop with a
`tokio::select!` over `TcpListener::accept()` and a `oneshot` shutdown channel
(`lib.rs`), and the two platform variants of the same for UDS and named pipes
(`ipc.rs`). Per connection: one `run_socket_task`, a `tokio::select!` between
`AsyncReadExt::read_buf` and an unbounded `mpsc` receiver. Per socket: that
`mpsc` command channel, which every `write` / `end` / `destroy` / `setNoDelay`
travelled through to reach the kernel.

Those paths still exist and are still exercised — by TLS, by outbound TCP
clients and by worker agents — so this is a narrowing, not yet a removal.
`SocketState::command` is the single choke point that picks a transport, so
neither can be reached by accident, and `SocketState::turnloop` is decided once
at creation and never changes.

## API and FFI

New in perry-runtime: `crates/perry-runtime/src/turnloop_net/`.

- `mod.rs` — the loop-owned socket layer: a handle table keyed by the binding's
  own JS-visible id, multishot accept and read, ordered writes with a queued-byte
  count, write-side shutdown, exactly-once close, `ref`/`unref`, and hostname
  resolution off the loop thread.
- `errors.rs` — `turnloop::Error { kind, os }` → Node's `code`/`errno`/`syscall`.
  Keyed on the host's own `libc::E*` / `WSAE*` values, so darwin, linux and
  windows are correct from one table; the portable `ErrorKind` is the fallback
  for a failure that never reached a syscall.
- `sink.rs` — the subsystem registry. A binding installs one completion sink and
  one id allocator (an accepted connection has to be named in the binding's
  handle space, which perry-runtime cannot allocate from).
- `abi.rs` — the C ABI, shaped like the event pump's existing registration
  surface, because a binding is a separately linked `staticlib` with no Cargo
  edge to perry-runtime and cannot hold a `&mut Loop`.

`perry-ffi::turnloop_net` is the safe Rust face of that ABI. Both sides declare
the completion struct independently, so both compute a layout digest from their
own definition and registration is **refused** if they disagree — the failure
mode otherwise is reading a byte count out of a pointer field.

Routing needs no side table: the submission `Token` carries the operation class
in its top 8 bits and the Perry-side id in the low 56, so a completion names its
socket and its syscall without a lookup, and a stale token from a closed socket
finds no entry and is dropped.

### Loop sizing

P0 created the loop with 16 handles and no buffer pool, which is right for a
program that only waits. The loop is now created at that profile and **upgraded**
— recreated — at the first net submission, to 4096 handles and 64 × 16 KiB
pooled read buffers. The upgrade only ever runs while the loop owns no handles,
which is asserted rather than assumed. A timer-only program keeps P0's footprint.

## GC decisions

**No JS heap memory is handed to the driver, ever.** Reads land in turnloop's
own pooled buffers and are copied into JS values by the sink, on the owning
thread, inside the dispatch call; writes arrive as an owned `Vec<u8>` that
`jsvalue_to_socket_bytes` had already copied out of the JS value. So there is no
buffer to root across a collection and no pointer for a moving collector to
invalidate — strictly stronger than DESIGN D3's "root from submit to
completion", and the reason this module registers no GC root scanner of its own.

That costs one 16 KiB memcpy per read relative to the tokio path, which read
straight into the binding's pooled `BytesMut`. Recovering it means passing that
buffer to turnloop as `ReadBuf::Provided`, which is an unsafe lifetime contract
across the C ABI; it is a deliberate follow-up, not an oversight.

Completions are dispatched **after** `turn` has returned (DESIGN D1), out of a
staging buffer, with no borrow held on the loop — so a `'data'` listener may run
JS, allocate, collect and submit new work on the same loop. The entry, with its
queued writes, is dropped only on the handle's final `Closed` (DESIGN D4).

The binding's existing GC surface is untouched: listener closures, write
callbacks and in-flight dispatch frames stay under `perry-ext-net`'s own
registered scanner, because none of that moved.

## Behaviours that needed explicit handling

Three things the tokio task got from its structure and a completion model does not.

1. **`'end'` before the `'connection'` callback.** turnloop can deliver a
   request and its FIN inside the first turn after accept, and `server_state`
   defers a loopback `ServerConnection` across a pump boundary on purpose — so
   an `'end'` pushed at EOF time reached a socket with no listeners yet and was
   lost. It is now held until the ServerConnectionReady marker, which is the
   same marker the tokio task blocked its post-EOF drain on. (Data already had
   this treatment; end did not, because the tokio transport never produced one
   that early.)
2. **Close must not cancel the `'end'` handler's writes.** `Loop::close` cancels
   outstanding operations. The post-EOF auto-end therefore submits the shutdown
   and closes only when *that* completes — turnloop orders a handle's writes
   ahead of its shutdown, so a completed shutdown means every queued byte left.
3. **A transient accept error does not end the listener.** The tokio accept loop
   deliberately did not break on one, and Node does not either. Completions
   carry a `terminal` flag so the binding can tell the two apart.

## Test evidence

All commands as run.

### Runtime unit tests — real sockets, on the real driver

```
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-runtime turnloop_net
```
→ **15 passed**, on macOS arm64 (kqueue) and on Linux x86_64 (epoll). They are loopback tests against the actual `Loop`, not mocks:
a full TCP exchange in both directions with byte assertions; half-close, with
the queued write proven to precede the FIN and the reader proven to survive its
own half-close; three writes submitted before a single turn, asserted to
complete in submission order and to drain the queued-byte count to zero; a
refused connect asserting `ECONNREFUSED` / `connect` / a negative errno; a
hostname connect that only passes if a refused first address falls through to a
reachable family; an unresolvable name asserting `ENOTFOUND` / `getaddrinfo`
and that no pending connect leaks; a Unix-domain socket round trip; a close with
a queued write asserting exactly one `Closed`; and rejection of submissions for
an unknown id. Each pairs its byte assertion with a completion-kind assertion,
and the loopback tests check `live_handles()` so a run that created no socket
cannot pass.

```
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-ext-net --lib   → 36 passed
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-ffi             → 39 passed
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-runtime event_pump → 22 passed
```

### Gap suite

`test-files/test_gap_turnloop_net_sockets.ts` is new: a TCP listener on an
ephemeral port with an echoing accepted connection, half-close, a Unix-domain
socket round trip, a refused connect's `code`/`syscall`, and a 1 MiB queued-write
workload. It prints no port, path or errno, because those are host-specific and
asserting them would make the test about the platform rather than the behaviour
— errno in particular is 61 on darwin and 111 on linux for the same
ECONNREFUSED.

Unix-domain sockets had **no** end-to-end coverage in the repository before this
(the only mention was a `net._normalizeArgs` string check), which is why the UDS
half is in the same test rather than waiting for its own.

```
export PATH=/opt/node-v26.5.1-linux-x64/bin:$PATH
export PERRY_SKIP_BUILD=1 PERRY_RUNTIME_DIR=$PWD/target/release
./run_parity_tests.sh --filter test_gap_turnloop_net
```
→ **Parity Pass 1, Fail 0, Crashed 0**, byte-identical to the oracle.

`PERRY_RUNTIME_DIR` is not optional on that host: `/etc/profile.d/perry.sh`
exports it pointing at a *different* checkout, so a run that does not override
it links someone else's archives. The first sweep here did exactly that and its
results were discarded.

The same command over every other net and IPC test in `test-files/`:

| filter | pass | fail | compile-fail | crash |
|---|---|---|---|---|
| `test_gap_turnloop_net` | 1 | 0 | 0 | 0 |
| `test_gap_net` | 2 | 0 | 0 | 0 |
| `test_gap_gc_net` | 1 | 0 | 0 | 0 |
| `test_net_` (incl. `test_net_upgrade_tls`) | 4 | 0 | 0 | 0 |
| `test_issue_1852` (net lifecycle) | 1 | 0 | 0 | 0 |
| `test_issue_2131` (net lifecycle edge) | 1 | 0 | 0 | 0 |
| `test_issue_422` (socket connect) | 1 | 0 | 0 | 0 |
| `test_issue_1123` (createServer / listen) | 2 | 0 | 0 | 0 |
| `test_issue_1131` (socket.write types) | 1 | 0 | 0 | 0 |
| `test_issue_5021` (write from a data handler) | 1 | 0 | 0 | 0 |
| `test_issue_647` (await socket event) | 1 | 0 | 0 | 0 |
| `test_parity_net` | 1 | 0 | 0 | 0 |
| `test_issue_1933` (fork IPC) | 1 | 0 | 0 | 0 |
| `test_sock_write` | 1 | 0 | 0 | 0 |

### node-suite `net` — the 47-fixture behavioural corpus

This is the real gate for this change, and it was run **against a baseline built
from this branch's own pre-migration commit** (`956384fc14`: the P1 core exists,
nothing uses it), because a corpus that is partly red at baseline cannot be read
from one arm.

```
export PATH=/opt/node-v26.5.1-linux-x64/bin:$PATH PERRY_SKIP_BUILD=1
PERRY_RUNTIME_DIR=<tree>/target/release ./run_parity_tests.sh --suite node-suite --module net
```

| | pass | parity-fail | crash | total |
|---|---|---|---|---|
| baseline `956384fc14` | 16 | 27 | 4 | 47 |
| P1 | **17** | 26 | 4 | 47 |

Per-test, **exactly one row changed**: `net/connection/data-roundtrip` went
`parity_fail` → `pass`. The four crashes are the same four fixtures in both arms
(`connection/address-metadata`, `exports/class-prototypes`,
`method-values/server-async-dispose`, `server/get-connections`), and every other
fixture kept its status. The committed floor for this module is pass ≥ 16 of 47
(`test-parity/node_suite_baseline.json`), so this is one above it.

One of those crashes was investigated far enough to attribute it:
`connection/address-metadata` fails because `socket.localAddress` /
`remoteAddress` / `localPort` are `undefined` on an **accepted** socket. That is
**pre-existing** — the same probe returns the same `undefined` on the baseline
build, where the accepted socket is a tokio socket whose `SocketState` does get
its endpoints from `TcpStream::local_addr()`. The turnloop path records the same
endpoints (asserted directly in the runtime unit tests, on both macOS and
Linux), so the loss is somewhere in the accepted-socket property dispatch and
predates this work. Worth its own issue.

### GC stress with I/O pending

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 ./gapnet
```

Clean, with the full 15 lines of output, and the instruments prove they were
armed rather than merely quiet:

- **486** `[gc-fromspace-protect] retired_set=#N` lines, so copying minors
  really ran and their from-space really was quarantined and `mprotect`ed — a
  run with zero copying minors protects nothing and would have passed vacuously;
- 17,157 `[gc…]` diagnostic lines over the run;
- `completions=99` on the same run, so those collections landed while socket
  operations were in flight;
- no SIGSEGV from the quarantine reporter: no stale from-space pointer was
  dereferenced.

Seeds 1, 7 and 12345 all pass at `RATE=1` (a collection at every handled
safepoint) with `ALLOC_KB=0` (every loop poll a candidate).

## `PERRY_LOOP_STATS` for a net workload

P0 measured that a Perry server made **0 turnloop turns**, because the accept
loop pinned a tokio task and the park always chose the tokio tick. After P1, the
same class of workload turns the loop and dispatches completions on it.

A turnloop-backed server answering a Perry client (both sockets in one process,
so the client is still a tokio socket):

```
[perry-loop] driver=turnloop turns=8 os_waits=0 zero_event_waits=2 \
             native_ticks=2 turn_errors=0 completions=7
```

The whole gap test (four scenarios, 1 MiB of queued writes):

```
[perry-loop] driver=turnloop turns=737 os_waits=0 zero_event_waits=709 \
             native_ticks=712 turn_errors=0 completions=98
```

`completions=` is new in this phase and is the load-bearing number: turns alone
would be satisfied by an idle loop, while a completion can only exist if
turnloop actually carried a socket operation.

`native_ticks` is still high because those runs have tokio sockets in the same
process — see the next section. A server-only workload is the arm to measure for
the A/B; that measurement has not been taken yet.

## The mixed-transport cost (transitional)

P0's park picked **one** wait: the tokio tick while tokio owned native work,
otherwise a turnloop turn. P1 creates the case that choice cannot cover — a
tokio socket and a turnloop socket live at once, which is now the normal shape
because `net.connect` clients stayed behind. A full-budget tokio tick then never
returns to collect a turnloop completion, and since that completion is what
would have produced the notify that ends the tick, the two **deadlock** rather
than merely delay each other. A turnloop-backed server answering a Perry client
hung after `'end'` until this was fixed.

While both transports are live the tick now takes a **1 ms slice** and the loop
is turned immediately after, so neither waits on the other for longer than that.
When only one is live nothing changes: a turnloop-only program still blocks to
its exact deadline in one turn, and a tokio-only program still gets the full
budget. One millisecond is the pre-P0 loop's own floor, so a mixed program is no
coarser than Perry was before this work.

This is a timer where it should be a readiness edge. The proper bridge is to
register turnloop's `Integration::Fd` (unix) / `Integration::Event` (Windows)
inside the tick so it ends when turnloop has work; P2–P7 remove the second loop
entirely and with it this branch.

## Remaining tokio in the `net` path

Every one of these is still reachable and still exercised, which is why the
tokio transport is narrowed rather than removed.

| site | what still uses tokio |
|---|---|
| `lib.rs` `spawn_socket_task_initialized` | the outbound TCP connect and its `run_socket_task` |
| `lib.rs` `run_socket_task` | the read/command loop for every socket that stayed |
| `ipc.rs` `spawn_listener` / `run_listener` | the local accept loops, now only the fallback when the agent has no loop |
| `ipc.rs` `spawn_connect` (tokio branch) | same fallback for a local client |
| `tls.rs`, `transport.rs` | `tokio_rustls`, `Transport::Tls`, and the mid-stream upgrade |
| `tls.rs` `schedule_tls_abort` | a 25 ms `tokio::time::sleep` that defers an aborted TLS connect's error |
| `server_state.rs` `schedule_server_connection` | a 1 ms `tokio::time::sleep` that orders a loopback `'connection'` against the client's `'connect'` |
| `adopt.rs` | an HTTP `'upgrade'` handing its live `TcpStream` to `net` |
| `perry-stdlib/src/net/` | the whole bundled implementation (compiled out by default) |

The two `sleep`-based ordering helpers are worth noting for P3: they are timers
being used as sequencing, and they will want the turnloop timer heap rather than
tokio's.

## Known gaps

- **`socket.setNoDelay()` cannot reach the kernel on a turnloop socket.**
  turnloop only accepts `TcpOpts { nodelay }` at socket creation (alpha.2 and
  alpha.3 both), and `ListenOpts` has no equivalent, so an accepted connection
  has no way to set `TCP_NODELAY`. The call keeps Node's chainable semantics and
  the flag is not observable from JS, but the kernel state differs from the
  tokio path, which set it on every accepted socket. **Needs a turnloop
  socket-option API.** Note the previous behaviour was itself divergent: Node
  does not set `TCP_NODELAY` by default.
- **`socket._handle.fd` is `undefined` on a turnloop socket**, for the same
  reason as the TLS handoff — turnloop does not expose the descriptor. Claude
  Code reads this through Node's private shape before the read-only `Bun.ant`
  peer-credential hooks.
- **One extra 16 KiB copy per read** (see GC decisions).
- **The P0 branch does not build on Linux.** turnloop alpha.2's exact `libc
  =0.2.175` pin predates `backtrace_symbols_fd`, which
  `arena/quarantine.rs` and `exception.rs` both call, so `perry-runtime` fails
  to compile there. Found while building the baseline; the alpha.3 bump in this
  branch fixes it by restoring libc 0.2.189. Worth knowing before anyone tries
  to bisect across P0 on a Linux runner.
- The bundled stdlib `net` (`crates/perry-stdlib/src/net/`) is untouched. It is
  compiled **out** of default builds by the well-known flip
  (`crates/perry/src/commands/compile/optimized_libs/driver.rs` strips
  `bundled-net`), is client-only, and only links under
  `PERRY_DISABLE_WELL_KNOWN=1`. It is P1 work that remains, and it can reuse this
  core through a second subsystem slot.
- `child.send(msg, handle)` fd/handle passing was **not** touched: it lives in
  `crates/perry-runtime/src/child_process/`, not in `net`, and belongs with P2's
  child_process migration. turnloop's `send_handle`/`recv_handle` are the API it
  will use.

## turnloop dependency

Bumped to **0.1.0-alpha.3** (published 2026-09-15T09:37:32Z, checksum
`c3370511f37b90dc5ba694566cb941f0e89c205c84b798277f17a05f13e4a8ab`), resolved
once with `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow` and then built
`--locked`. The workspace requirement is a caret, not an `=` pin: the exact
version is the lockfile's job, and an exact requirement here would propagate
exactly the problem alpha.3 fixed.

alpha.3's caret requirements lift the forced downgrades alpha.2 imposed, and the
lockfile is restored to the versions `main` resolved before P0: libc
0.2.175 → 0.2.189, tokio 1.50.0 → 1.53.1, redis 1.2.4 → 1.6.0, wasm-bindgen
0.2.108 → 0.2.122 (with js-sys, web-sys, wasm-bindgen-futures and the macro
crates), mio 1.1.0 → 1.2.1, rustix 1.1.2 → 1.1.4, linux-raw-sys 0.11.0 → 0.12.1,
tempfile 3.23.0 → 3.27.0, and num-bigint 0.5.1 back with redis. What remains
added over pre-P0 is turnloop itself plus `loom` and `generator`.

alpha.3 also widened `ErrorKind` with filesystem categories for its typed file
operations; the Node mapper covers them explicitly rather than folding them into
`UNKNOWN`, so P2's pipes and P4's file jobs inherit a real code.

## One gate artifact the integrator should look at

`scripts/gc_runtime_root_holders.py` changes verdict on a holder in a crate this
work never touched: `perry-ext-http`'s `HTTP_PENDING_EVENTS` flips from
UNCOVERED to COVERED, which makes its inventory entry stale, and a stale entry
fails the gate — so the entry is deleted here.

The cause is name resolution, not a new scanner. Adding
`crates/perry-ext-net/src/turnloop_io.rs` (confirmed by removing just that file
and re-running: the verdict flips back) puts another reachable body in a
registering crate that calls `push_event`. `push_event` is also the name of a
function in `perry-ext-http/src/lib.rs`, and the walk resolves names across
crates, so ext-http's `push_event` body joins its own file's reachable text —
and that body is the one that mentions `HTTP_PENDING_EVENTS`.

Nothing about the holder changed: its recorded verdict was already
`not_a_gc_pointer` ("no NaN-boxed value"; the closures live in
`ClientRequestHandle`, which `scan_http_roots` visits), and that still holds.
What is lost is the *record* of that reasoning, because the gate has no way to
keep an entry for a holder it now considers covered. If the walk were resolved
per crate — a scanner only calls within its own crate or into perry-ffi, which
the script's own comment already says — this class of coincidence would go away.

## For the integrator

- Full gap suite (fast and auto-optimize tiers) and `cargo test --workspace`.
- `./run_parity_tests.sh --suite node-suite --module net` — the 47-fixture net
  corpus, which is the real behavioural gate for this change and is far broader
  than the gap tests.
- GC stress with `PERRY_GC_SCHEDULE_SEED` + `PERRY_GC_PROTECT_FROMSPACE` over a
  net workload, asserting collections landed while I/O was pending.
- A cgu=1 instruction A/B with a control probe, on a **server-only** workload so
  the mixed-transport slice does not dominate. The tokio arm is
  `--features perry-stdlib/tokio-wait-driver`, which turns the loop off and puts
  every socket back on tokio.
- A Windows arm. Named pipes, `ListenOpts`, and the Windows half of the error
  table have not been exercised.
- The baseline tree is still on the build box at `/root/claude-turnloop-p1-base`
  (commit `956384fc14`, its own `target/`), next to the working tree at
  `/root/claude-turnloop-p1`. Delete both when the A/B is done. Both need
  `PERRY_RUNTIME_DIR` overridden per the note above, and Node 26.5.1 was
  installed at `/opt/node-v26.5.1-linux-x64` because the box only carried
  26.8.1.
