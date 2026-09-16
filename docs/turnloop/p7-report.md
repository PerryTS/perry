# turnloop P7 — the database drivers

Branch `turnloop/p7-databases`, based on `turnloop/integration` at `7f77cce3c6`
(P0 through P5 plus `main` through v0.5.1576). Built and tested on the shared
Linux box (`perrybuilder`, EPYC 9354P) against the pinned gap oracle Node
**26.5.1** (`/opt/node-v26.5.1-linux-x64/bin`, not the box default 26.8.1), with
real PostgreSQL 16.15, MySQL 8.0.46, Redis 7.0.15 and MongoDB 8.0.32 servers. Nothing
here was run on Windows or macOS, and nothing was benchmarked.

## The finding, before the change

P4's report named the shape: every database binding held a tokio blocking-pool
thread for the duration of every call —

```rust
perry_ffi::spawn_blocking(move || {
    tokio::runtime::Handle::current().block_on(async move { conn.query(..).await })
})
```

— and turnloop's pool is bounded and fixed-size by design, so an occupant of
that shape cannot be rehosted on it. That is why `spawn_blocking` survived P4.

Measured rather than argued. Sixteen `ioredis` clients, one command in flight on
each, on the base commit:

```
connections: 16
idle threads: 1
in-flight threads: 17 | p7-census-base x1, tokio-rt-worker x16
acks: 16 all-OK
reads: 16 of 16
after threads: 17 | p7-census-base x1, tokio-rt-worker x16
closed threads: 17
[perry-loop] driver=turnloop turns=0 … completions=0 …
[perry-loop-waits] … tokio_ticks=1 …
```

One tokio thread per in-flight command, and they persist after the work
finishes. The loop made **zero** turns: turnloop carried nothing.

## What moved, and what did not

| surface | transport after P7 | why |
|---|---|---|
| `ioredis` client, plaintext | **turnloop** + `turnloop-redis` | — |
| `pg` `Client`, plaintext | **turnloop** + `turnloop-postgres` (SCRAM-SHA-256) | — |
| `pg` `Pool`, plaintext | **turnloop**, one connection | see "the pools" |
| `mysql2` connection + pool, plaintext | **turnloop** + `turnloop-mysql` | — |
| `mongodb`, direct single-host plaintext `mongodb://` | **turnloop** + `turnloop-mongodb` | — |
| any client on a `worker_threads` agent | legacy | that agent has no loop of its own |
| any client under `--features perry-stdlib/tokio-wait-driver` | legacy | the loop is off |
| `rediss://` (and `REDIS_TLS` unset, which defaults to **true**) | legacy | no TLS layer reachable from a database binding |
| `mongodb+srv://`, `tls=`, several hosts, `replicaSet=`, `compressors=` | legacy | SRV, topology discovery and rustls stay in the `mongodb` driver |
| a Unix-domain-socket `pg` host | legacy | the driver submits a TCP connect; a socket path needs `pipe_connect` |
| `perry-stdlib`'s own `mysql2`/`pg`/`ioredis`/`mongodb` modules | untouched | compiled **out** by the well-known flip in any default build |
| `better-sqlite3`, `bun:sqlite` | untouched | not a network driver |

This is a narrowing, not a removal. `sqlx`, the `redis` crate and the `mongodb`
driver all stay, and the declining rows are reachable configurations — the same
shape P1 and P5 left their fallbacks in.

One qualification, because the table would otherwise overstate it: the
`rediss://` row declines to a path that **cannot work either**, since this
crate's `redis` dependency has no TLS backend compiled in. Declining preserves
today's failure rather than a working configuration, which is still the right
answer for a transport migration but is not the same claim.

## The result, measured

The same fixture, same host, one compiler apart:

| | base `7f77cce3c6` | P7 |
|---|---|---|
| idle threads | 1 | 1 |
| 16 connections, 16 commands in flight | **17** — `tokio-rt-worker x16` | **1** |
| after the work finished | 17 | **1** |
| turnloop `turns` | 0 | **105** |
| turnloop `completions` | 0 | **304** |
| `native_ticks` | 1 | **0** |
| `tokio_ticks` | 1 | **0** |
| `acks` / `reads` | 16 all-OK / 16 of 16 | 16 all-OK / 16 of 16 |

Sixteen connections now cost sixteen descriptors and the agent's own thread.
`tokio_ticks=0` says the loop never had to drive the legacy tick at all for this
workload: nothing in the process held a tokio task.

`scripts/turnloop/apps/db_thread_census.ts` is that fixture. It writes its
sixteen clients out as sixteen `const`s rather than an array on purpose — a
method call whose receiver is an array element does not resolve to Perry's
native-method table and silently returns `undefined`, on the base commit as well
as on this branch (see "Perry defects this work found").

## Architecture

```
tcp_connect ─► NET_CONNECT ─► core.transport_connected() ─┐
NET_DATA ───► core.receive(bytes) ────────────────────────┤
NET_TIMER ──► core.handle_timeout() ──────────────────────┤
                                                          ▼
                                                    core.drain()
                                         ┌────────────────┴───────────────┐
                                         │ settle each finished operation │
                                         │  JsPromise::resolve_with(…)    │
                                         └────────────────┬───────────────┘
                                                          ▼
                                    flush: write(core.output()), arm next_timeout
```

`crates/perry-db-turnloop` is that loop, once, for all four bindings.
`crates/perry-ext-*/src/turnloop_io*` is the per-protocol half: the `DbCore`
impl, the operation queue, and the wire→JS conversion.

Four rules hold it together.

1. **The sink runs no JS.** It runs inside the loop's completion dispatch, after
   a turn has returned (DESIGN D1), so it may allocate Rust state and settle
   promise tokens — but every result crosses to the main thread as owned Rust
   data inside a `JsPromise::resolve_with` closure, which the resolution pump
   invokes there. That is the #1824 rule the `spawn_blocking` bindings already
   had to obey, now with no worker thread involved at all.
5. **No JS value and no heap pointer reaches the driver.** Reads are copied out
   of turnloop's pooled lease inside the dispatch call; writes are handed over
   as owned `Vec<u8>`. P1's rule, unchanged, which is why this module registers
   no GC root scanner of its own.
6. **A transport is decided once, at client construction, and never changes.**
   P1's rule for sockets, for the same reason: whether a client is TLS or which
   agent owns it is not knowable later, and a client that switched mid-life
   would have two different connections to the same server.
7. **Every accepted operation gets exactly one settlement.** Including the
   failure paths — a dropped `JsPromise` is a promise that never resolves and
   never rejects, which is the one outcome a caller cannot recover from.

### Why not the crates' `asynchronous` modules

Each protocol crate ships one, and it would have been far less code. They need a
`turnloop_io::ExecutorHandle`, and P5's report explains why that is unusable
from a host that owns its own loop: `LocalExecutor::with_config` constructs its
own `Driver`, and `Shared::dispatch` returns early for any token without its own
tag bit, so P1's net tokens, P2's process tokens and P3's timer token would be
silently dropped. With `default-features = false` these four crates depend on
neither `turnloop-io` nor `turnloop-tls`; they are pure protocol state machines,
which is exactly what a host driving them over P1's completion layer wants.

### Subsystem slots

`turnloop_net`'s `MAX_SUBSYSTEMS` rises from 4 to 8. Slots: 0 `perry-ext-net`
(P1), 1 `perry-ext-http` (P5), 3 the runtime's own test slot, and 2/4/5/6 the
four database bindings. Four are needed rather than one because each binding is
a separately linked `staticlib` with its own completion sink — they cannot share
a slot even though they share this module. `register_sink` refuses an
out-of-range slot, and `the_four_subsystem_slots_are_distinct_and_clear_of_p1_and_p5`
pins the allocation so a fifth binding cannot quietly take an occupied one.

## Decisions this lane had to make, and what they cost

### Connection pooling

| | before | after |
|---|---|---|
| `pg` `Pool` | `sqlx::PgPool`, `max_connections(10)` hardcoded, a connection checked out **per call** | one loop-driven connection, commands pipelined |
| `mysql2` pool | `sqlx::MySqlPool`, `max_connections(10)`, 10 s acquire timeout, one connection per request | bounded FIFO of up to 10 loop-driven connections, 10 s acquire deadline, one connection per request |
| `ioredis` | one cached `MultiplexedConnection` per client | one loop-driven connection per client |
| `mongodb` | the driver's own per-server pool | one loop-driven connection per client |

No JS-visible pool option was honoured before this change and none is honoured
after it: `connectionLimit`, `queueLimit`, `waitForConnections`, `idleTimeout`,
`min` and `max` are read by neither config parser, on either transport. What
**is** lost, named plainly:

* **`pg`'s pool no longer opens more than one connection**, so a program issuing
  concurrent pool queries loses server-side parallelism. It gains ordering:
  sqlx's pool checked out per call, so `pool.query('BEGIN')` followed by another
  `pool.query` could land on two different backends and silently break the
  transaction. Neither shape is node-postgres's; a real pool wants
  `turnloop_postgres::pool`, whose host-executed Connect/Close events the shared
  driver does not yet expose.
* **`mysql2`'s pool has no idle reaper.** sqlx's did. A pool that peaks at ten
  keeps ten sockets until `pool.end()`.

### Transactions and ordering

A transaction pins a connection on every migrated driver, because a `Client` /
`Connection` / `PoolConnection` handle owns exactly one loop-driven connection
for its whole life, and `mysql2`'s `getConnection()` pins a pool member until
`release()` — which waits for outstanding work before returning it. `BEGIN` …
`COMMIT` on a bare `pg`/`mysql2` **pool** is exactly as unreliable as it was
before, for the same reason, and is not something this change fixes.

Ordering is a property of the transport rather than of the bindings.
`turnloop_net` orders a handle's writes, and the driver acknowledges a core's
output with `consume_output` only once turnloop has taken ownership of the
bytes — so nothing encoded afterwards can overtake them. That is what makes a
Redis pipeline and a `MULTI`/`EXEC` block reach the wire in submission order.
The fixture checks it rather than asserting it: five commands submitted before
any is awaited, with the reply of the first paired against its own promise.

MySQL and MongoDB have no pipelining at all — `turnloop_mysql::Connection` holds
one pending command and refuses a second, and `turnloop_mongodb` is explicit
request/response — so both cores own a **queue** and issue the next command only
after the previous `Completed`. That queue is the single most important
correctness property in those two files and each has a test that fails if it
reorders or loses a submission.

### TLS to the database

**Not implemented, and nothing regresses.** Perry had no database TLS before
this change on any of the four:

* `pg`: `parse_pg_config` never read an `ssl` field and no `sqlx` TLS backend
  was compiled in;
* `mysql2`: `to_url()` hardcodes `?ssl-mode=disabled`;
* `ioredis`: the `redis` dependency has no TLS feature, so a `rediss://` URL
  cannot connect at all — **and that is the default**, because `REDIS_TLS`
  defaults to `true` (see the defects section);
* `mongodb`: the only one with real TLS, via `rustls-tls` — which is why a
  `tls=`/`+srv` URI declines to the existing driver rather than being migrated.

So a TLS client keeps its legacy path and fails, or succeeds, exactly as it does
today. PostgreSQL's SCRAM-SHA-256-**PLUS** channel binding is therefore also out
of scope: `turnloop-postgres` derives `tls-server-end-point` from the verified
leaf certificate and the binding refuses a `plus` request rather than answering
it with a bogus binding. Doing this properly wants the unbuffered-rustls layer
P5 built inside `perry-ext-net` to be reachable from a database binding; it is
not, today.

### Compression

`turnloop-mysql` and `turnloop-mongodb` 0.1.0-alpha.4 is the release that gave
compressed commands **one deflate state per connection, each message framed as
its own zlib stream** — alpha.3 re-created the state per message. This branch
depends on alpha.4 for exactly that reason, and the workspace manifest says so.

Compression is nevertheless **off** on both. Perry exposes no compression option
on either driver and never has, so turning it on would be a wire change with no
caller — the untested-mode shape CLAUDE.md's GC knob kill-policy warns about.
The fix is adopted, not exercised.

### Cursors and streaming results

Nothing streamed before this change and nothing streams after it. Every
migrated path buffers:

* `pg` and `mysql2` called `fetch_all` — the whole `Vec<Row>` before any
  conversion. The turnloop path collects owned rows in the core and converts
  them in one `resolve_with`.
* `mongodb`'s `find` called `try_collect()` on the cursor. The turnloop path
  **follows the cursor with `getMore`** until it is exhausted, which is a
  correctness requirement rather than a feature: a transport that stopped at the
  first `OP_MSG` batch would silently return the server's default 101 documents.
  That continuation is **implemented but not verified end to end**, and the
  reason is defect 3 below: `find().toArray()` resolves an empty string on both
  arms, so no fixture can observe how many documents the cursor produced.
  `mongo_parity.ts` therefore inserts 250 and checks the *server's* count, which
  proves the write half and says nothing about the read half.

A streaming API is a JS-surface change (`query().stream()`, a real cursor
object) and belongs in its own phase.

## Every remaining tokio-reachable database path

Named exhaustively, because "the drivers moved off tokio" is not true and the
difference matters to whoever deletes the dependency in P8.

| site | what still runs on tokio | reachable when |
|---|---|---|
| `perry-ext-ioredis` `dispatch` / `get_connection` | `redis::aio::MultiplexedConnection` + `spawn_blocking` + `Handle::block_on` | the client declined at construction: no loop on this agent, the `tokio-wait-driver` arm, or `REDIS_TLS` not `false` |
| `perry-ext-pg` (all ten entry points' legacy arm) | `sqlx::postgres` + `spawn_blocking` + `Handle::block_on` | ditto, plus a Unix-domain-socket host |
| `perry-ext-mysql2` (all ten legacy arms) | `sqlx::mysql` + `spawn_blocking` + `Handle::block_on` | ditto |
| `perry-ext-mongodb` (every entry point's legacy arm) | the `mongodb` driver (its own pool, SDAM monitors, rustls, hickory DNS) | ditto, plus `+srv`, `tls=`, several hosts, `replicaSet=`, `compressors=`, an unparsable URI, or no `/dev/urandom` |
| `perry-stdlib/src/{ioredis.rs, mongodb.rs, mysql2/, pg/}` | `sqlx` / `redis` / `mongodb` on the shared **current-thread** runtime (cooperative `.await`, no thread per call) | only under `PERRY_DISABLE_WELL_KNOWN=1`, or when the ext crate's source is absent from disk; compiled out of every default build |
| `perry-ext-better-sqlite3`, `perry-stdlib/src/sqlite/`, `bun_sql.rs` | nothing — SQLite is in-process | always; there is no transport here to move |

Two things follow. First, **the tokio blocking pool is still reachable from a
database binding** — a `worker_threads` agent takes the legacy path for all
four, which is the same hole P4 left for its own subjects and which per-agent
loops close rather than this phase. Second, the stdlib copies are the *only*
database code that never needed a thread per call; they were already cooperative
on the shared runtime, and they are also the copies nobody links.

## GC decisions

* **No new root scanner, and the reason is structural**: no JS value and no heap
  pointer reaches the driver (rule 2 above).
* **A `JsPromise` parked in a core's pending table is a raw `*mut Promise` in a
  side table**, which is exactly the shape `scripts/gc_runtime_root_holders.py`
  exists to catch — so it is classified rather than left to be discovered.
  `perry_ffi_promise_new` is `js_native_async_completion_new` +
  `js_native_async_completion_promise`, so every one of those promises was
  minted by `js_promise_new_cross_thread`: **pinned at creation in non-moving
  malloc space and rooted by its native-async token until settlement**
  (#9356, #9552). The address neither moves nor goes unrooted. That is the same
  contract `perry-ext-net`'s P5 `LAYERS` records, reached through perry-ffi's
  promise constructor rather than through an explicit token. Eight verdicts are
  written into `scripts/gc_runtime_root_holders.json`; the gate is green.
* **Retirement settles before it drops.** Both retirement paths — the driver's
  terminal `NET_CLOSED` and the `close`-already-gone branch — drop the entry and
  with it the core. Dropping without settling strands every promise the core
  still owes. The first version of this driver had that hole and two independent
  reviewers found it; it now has a test that also checks a *clean* close does
  not invent a rejection.

## Test evidence

Every command as run, on the shared Linux box.

### The servers, and how to reproduce them

Not available in a sandbox, so this lane installed and ran its own, on private
ports, in private data directories, as its own processes — no systemd units, so
another session's expectations do not move. The control script is
`/root/claude-turnloop-p7/dbservers.sh` on the build box (`init` / `start` /
`seed` / `stop` / `status`), with data under `/srv/claude-turnloop-p7-servers`.

| server | version | endpoint | credentials |
|---|---|---|---|
| PostgreSQL | 16.15 | `127.0.0.1:55432` | `perry` / `perry_test`, db `perry_test`, **scram-sha-256** |
| MySQL | 8.0.46 | `127.0.0.1:53306` | `perry` (caching_sha2) and `perrynat` (mysql_native_password) / `perry_test`, db `perry_test` |
| Redis | 7.0.15 (Ubuntu `redis-server`) | `127.0.0.1:56379` | none |
| MongoDB | 8.0.32 | `127.0.0.1:57017` | none |

Two things worth knowing before repeating this. `/root` is mode 700, so a
server that drops privileges cannot traverse into it — the data directories live
under `/srv`. And Ubuntu's `mysqld` is AppArmor-**enforced** with a profile
scoped to `/var/lib/mysql`, so a private data directory needs a local override
(`/etc/apparmor.d/local/usr.sbin.mysqld`) and `mysqld --initialize` needs the
directory to exist, empty, and owned by `mysql`.

The Node oracle's packages are installed separately, at
`/root/claude-turnloop-p7/oracle` (`ioredis@5 pg@8 mysql2@3 mongodb@6` under
Node 26.5.1), because Node resolves a bare specifier relative to the **file**,
not the working directory.

### Redis, byte-for-byte against the oracle

`scripts/turnloop/apps/redis_parity.ts`, compiled by Perry and run by
`node --experimental-strip-types` against the same server:

```
=== diff (perry vs node) ===
BYTE-IDENTICAL
```

Twenty-one lines covering `SET`/`GET`/`DEL`/`EXISTS`/`INCR`/`DECR`/`EXPIRE`, a
missing key (`null`, not `""`), a value with a multi-byte character and an
embedded newline, a 64 KiB value that spans several reads, and five commands
pipelined on one connection with the replies checked against their own promises.

Its liveness counters on the same run:

```
[perry-db] subsystem=5 connect id=6597069766656 127.0.0.1:56379
[perry-db] subsystem=5 closed id=6597069766656 connects=1 reads=33 writes=31 timer_arms=39 live=0
[perry-loop] driver=turnloop turns=60 os_waits=31 zero_event_waits=1 native_ticks=0 turn_errors=0 completions=121
[perry-loop-waits] arm=turnloop turnloop_waits=58 … tokio_ticks=0 …
```

`PERRY_DB_TURNLOOP_DIAG=1` is this lane's "did the subject run" instrument. A
green suite says nothing about which transport carried the workload, and a
migration whose subject never ran is the most dangerous of CLAUDE.md's four ways
a gate cannot fail. `connects=`, `reads=` and `writes=` are the positive answer.

`timer_arms=39` is a second instrument and it caught a real bug. It read **0**
on the first version: `flush` armed the core's deadline only once the transport
was up, which left exactly the case that most needs one — a connect that never
completes — with no timeout at all. Output still waits for the connect (a core's
handshake bytes are produced by `transport_connected`); the deadline no longer
does.

### GC stress with replies in flight

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 ./redis_gc_stress
```

Subject: `scripts/turnloop/apps/redis_gc_stress.ts` — four connections, four
commands submitted before any is awaited, an allocating loop between the
submission and the await, and a server-side error whose rejection must still
arrive at the end.

| seed | exit | copying minors | objects moved | from-space quarantines | loop polls | gc diagnostic lines | completions | stdout |
|---|---|---|---|---|---|---|---|---|
| 1 | 0 | 24,109 | 11,827 | 24,109 | 24,000 | 808,372 | 216 | byte-identical to the unstressed run |
| 7 | 0 | 24,109 | 11,827 | 24,109 | 24,000 | 808,372 | 216 | byte-identical |
| 12345 | 0 | 24,109 | 11,827 | 24,109 | 24,000 | 808,372 | 216 | byte-identical |

Re-run on the final build — after the retirement fix and after the other three
bindings landed — and every number above is unchanged.

All three seeds report identical counts, which is the documented behaviour at
`RATE=1`: every handled safepoint collects, so the seed stops selecting. No
SIGSEGV from the quarantine reporter: no stale from-space pointer was
dereferenced, while replies were outstanding and collections were moving
survivors.

**The stress fixture is deliberately separate from the parity fixture, and the
instrument is why.** Run against `redis_parity.ts` the same knobs exit **70**
with

```
[gc-schedule] THIS RUN EXERCISED NOTHING WORTH TRUSTING. … NOT ONE back-edge
poll was reached, so every collection came from an event-loop boundary and no
loop body was covered. Any "clean at rate 1" conclusion from this run is vacuous.
```

which is correct: that fixture has no allocating loop. Reporting it as a pass
would have been exactly the vacuous-gate failure the instrument exists to
prevent.

### PostgreSQL, byte-for-byte against the oracle

`scripts/turnloop/apps/pg_parity.ts`, against the **scram-sha-256** server:

```
=== diff (perry vs node) ===
BYTE-IDENTICAL
```

That is also the only evidence the host-side SCRAM handshake works: the core
asks for a `ScramSha256` because its constructor reads entropy, and the binding
builds it. Seventeen lines covering DDL, three inserts including an all-NULL
row, a `SELECT` of int4/text/bool/float8 with a multi-byte value, an empty
`SELECT`, a 70 000-byte value that spans several reads, `UPDATE`, `DELETE`, a
statement error that rejects and leaves the session usable, `BEGIN`/`ROLLBACK`,
`BEGIN`/`COMMIT`, and the `Pool` surface.

```
[perry-db] subsystem=2 connect id=3298534883328 127.0.0.1:55432
[perry-db] subsystem=2 connect id=3298534883329 127.0.0.1:55432
[perry-db] subsystem=2 closed id=3298534883328 connects=2 reads=33 writes=25 …
[perry-loop] driver=turnloop turns=70 os_waits=40 … native_ticks=0 … completions=72
[perry-loop-waits] … tokio_ticks=0 …
```

**The base arm cannot run this fixture at all**, and that is a finding rather
than a caveat. On `7f77cce3c6` every line reads

```
create: command=undefined rowCount=undefined rows=undefined
```

— the sqlx path's result object is unreadable from TypeScript — and the run then
dies on the same parameter defect (below). So for `pg`, P7 is the first time the
binding returns anything a program can use.

Two divergences from node-postgres are deliberately outside the fixture, both
reproduced on the base commit:

| | Node | base | P7 |
|---|---|---|---|
| `client.query(sql, params)` | works | `bind message supplies 0 parameters, but prepared statement "sqlx_s_8" requires 1` | `…but prepared statement "" requires 1` |
| `rowCount` on INSERT / UPDATE / DELETE | the affected count | `undefined` | `0` |
| `rowCount` on DDL | `null` | `undefined` | `0` |

The parameter defect is identical on both arms — the parameters never reach the
`Bind` message — so it is a pre-existing Perry defect and not something the
transport introduced. The `rowCount` row is inherited deliberately: the sqlx
path's no-parameter entry point called `fetch_all` for *every* statement and
reported `rows.len()`, so an `INSERT` reported zero, and the turnloop path
reproduces that choice rather than quietly changing a JS-visible value during a
transport migration. The real `CommandComplete` count is available from
`turnloop_postgres` and the binding already has the other shape
(`ResultKind::RowsAffected`) wired for the parameterized entry point; switching
the no-parameter one over is a one-line change that belongs in its own commit,
with its own oracle comparison.

### PostgreSQL, the same headline measurement

`scripts/turnloop/apps/pg_thread_census.ts`: twelve `pg` clients, each holding a
200 ms `pg_sleep` at the same moment.

| | base `7f77cce3c6` | P7 |
|---|---|---|
| idle threads | 1 | 1 |
| after 12 `connect()`s | **13** — `tokio-rt-worker x12` | **1** |
| 12 queries in flight | **13** | **1** |
| results correct | *the run fails*: `rows` is `undefined` | **12 of 12** |
| turnloop `turns` / `completions` | 0 / 0 | **42 / 108** |
| `native_ticks` | 2 | **0** |

Worth noting which row moved: the twelve threads on the base arm are consumed by
the **connects**, not by the queries — `js_pg_client_connect` is itself a
`spawn_blocking` — and they are still there after the connects have resolved.

### MySQL, byte-for-byte against the oracle, on both authentication plugins

`scripts/turnloop/apps/mysql_parity.ts`, run twice against MySQL 8.0.46:

| user | plugin | result |
|---|---|---|
| `perrynat` | `mysql_native_password` | **byte-identical** |
| `perry` | `caching_sha2_password` | **byte-identical** |

Fourteen lines covering `affectedRows`, a `SELECT` of INT/VARCHAR/TINYINT/DOUBLE
with an all-NULL row and a multi-byte value, an empty `SELECT`, **two prepared
statements with parameters** (`execute`, the binary result protocol), a
70 000-byte value, `UPDATE`, a statement error that leaves the connection
usable, and `beginTransaction`/`rollback`/`commit`.

The `caching_sha2` run was repeated after `FLUSH PRIVILEGES`, which clears the
server's password cache and forces **full** authentication — the path that emits
`RsaSeedNeeded` and needs 20 fresh random bytes from the host. It stays
byte-identical, and the transport counters show the two extra round trips:

```
# fast path (cache warm)   … reads=33 writes=27 timer_arms=36
# full path (cache flushed) … reads=33 writes=29 timer_arms=36
[perry-loop] driver=turnloop turns=60 … native_ticks=0 … completions=112
[perry-loop-waits] … tokio_ticks=0 …
```

### MySQL's pool, which had no unit test at all

Opening a pool member needs a real socket, so `pool.rs` is reviewed rather than
unit-tested. `scripts/turnloop/apps/mysql_pool_parity.ts` is the only
end-to-end exercise it gets, and it is **byte-identical** to Node:

```
pool-select: [{"id":1,"name":"one"},{"id":2,"name":"two"}]
pool-execute: [{"name":"two"}]
pool-concurrent: 1,2,3,4
inside-tx: [{"id":3}]
outside-tx: []
after-rollback: [{"id":1},{"id":2}]
after-commit: [{"id":1},{"id":2},{"id":4}]
done
```

`inside-tx` / `outside-tx` is the load-bearing pair. The pinned connection from
`getConnection()` has an open transaction holding row 3; the pool query issued
while it is still open sees **nothing** — so the pool really handed that query a
different physical connection, rather than the pinned one. `after-commit` then
proves the released member was returned usable. Four connections were opened,
all on one loop:

```
[perry-db] subsystem=4 connect id=5497558138880 127.0.0.1:53306
[perry-db] subsystem=4 connect id=5497558138881 127.0.0.1:53306
[perry-db] subsystem=4 connect id=5497558138882 127.0.0.1:53306
[perry-db] subsystem=4 connect id=5497558138883 127.0.0.1:53306
[perry-loop] driver=turnloop turns=60 … native_ticks=0 … completions=104
[perry-loop-waits] … tokio_ticks=0 …
```

### MongoDB, byte-for-byte against the base arm

`scripts/turnloop/apps/mongo_parity.ts`:

```
BASE vs P7: BYTE-IDENTICAL
```

**Not** byte-identical to Node, and deliberately reported that way: Perry's
MongoDB surface diverges from the npm driver's in ways that predate this change
and are unaffected by it. The complete list of lines that differ from Node, all
of them identical on `7f77cce3c6`:

| line | Node | Perry, both arms |
|---|---|---|
| `insertOne().acknowledged` | `true` | `false` |
| `insertMany().insertedCount` | `2` | `0` |
| `findOne(...)` | a document | a JSON **string** |
| `find().toArray()` | the documents | `""` |
| `updateOne().modifiedCount`, `updateMany().modifiedCount`, `deleteOne().deletedCount` | numbers | `undefined` |

Everything else matches Node exactly: `count`, `count-filtered`,
`count-after-delete`, `bulk-count: 250`, `bulk-count-filtered: 1`,
`count-after-clear: 0`, `find-one-missing: null`, and `findOne`'s payload once
the string quoting is accounted for. So the wire half works and the JS half is
the pre-existing gap. `find().toArray()` resolving an empty string is the most
serious of these — MongoDB's primary read API is unusable from TypeScript on
either transport — and it is also why the `getMore` continuation could not be
verified end to end.

```
[perry-db] subsystem=6 connect id=7696581394432 127.0.0.1:57017
[perry-loop] driver=turnloop turns=39 os_waits=19 … native_ticks=0 … completions=41
[perry-loop-waits] … tokio_ticks=0 …
```

### The full gap suite, against this branch's own base

Both arms built in their own tree with the harness's default package set plus
the four `perry-ext-*` database wrappers, in one cargo invocation, and run as
`PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh` against Node 26.5.1 on the same
box.

| | base `7f77cce3c6` | P7 |
|---|---|---|
| tests | 805 | 805 |
| parity pass | 796 | 796 |
| parity fail | 9 | **9 — the same nine** |
| compile fail | 0 | 0 |
| crash | 0 | 0 |
| **status changes vs base** | — | **0** |

The base's nine, none of them this lane's:
`2159_defineproperty_class_prototype`, `2514_settracesigint`,
`2899_2779_2777_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch`, `json_lazy_defineproperty_index`,
`perfhooks_3088_3008_3010_3011`, `prop_plan_cache_invalidation`,
`v8_2_3680plus`. Three of those — `2899_…`, `disposablestack_2875` and
`iterator_prototype_next_patch` — the committed snapshot expects to PASS, so the
gate is **red on the base commit before this branch changes anything**, which is
exactly why this comparison is against the base rather than against the
snapshot.

**Not one test changed status in either direction**, compared per test from the
two runs' own journals rather than from the summary lines. That is the verdict:
adding a workspace crate, raising `MAX_SUBSYSTEMS`, and rewriting the transport
inside all four database bindings cost the existing suite nothing.

No test in the suite opens a database connection, so this sweep is a regression
gate on everything *around* the change — the four bindings' shared archives,
the `MAX_SUBSYSTEMS` change in the runtime, and the new workspace crate — rather
than coverage of the migration itself. The migration's own coverage is the
fixtures above.


## Unit tests

```
CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo test -p perry-db-turnloop      →  9 passed
CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo test -p perry-ext-ioredis      →  9 passed
CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo test -p perry-ext-pg           → 21 passed
CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo test -p perry-ext-mongodb      → 12 passed
RUST_TEST_THREADS=1 … cargo test -p perry-ext-mysql2                               → 38 passed
RUST_TEST_THREADS=1 … cargo test --profile perry-dev -p perry-runtime turnloop_net → 15 passed
```

The environment variable is not optional: the four `turnloop-*` protocol crates
are inside the repository's 7-day `global-min-publish-age` soak window, so a
fresh resolve is refused without it. The committed `Cargo.lock` already carries
them, which is what lets auto-optimize — a plain `cargo build` — succeed.

Two things about these suites are worth naming.

`perry-ext-mysql2`'s 24 new tests drive a **synthetic MySQL server** written from
the wire format: a real handshake, real packet framing and sequence ids, text
and binary result sets. That is a real exercise of the core and of the command
queue, and it is still not proof against MySQL 8.0.46.

The registration tests assert `register`, not `enabled`. Whether the *thread a
test happens to run on* owns a loop is not a property of the build, and `cargo
test` puts each test on its own thread — the first version asserted `enabled`
and was flaky. What the split keeps is the load-bearing half: `register_sink`
refuses outright when perry-ffi's `NetCompletion` layout digest disagrees with
the runtime's, and a false there would silently put every client back on the
legacy transport.

## turnloop gaps found

Reported here in the shape P5's were, for the coordinator to file.

**`turnloop-postgres`**

1. **The host must keep a second copy of the password in memory.** The README
   *requires* the host to build `ScramSha256` at `ScramNeeded` time, because its
   constructor reads entropy — but `Connection::new` consumes the `Config` and
   exposes no accessor, so the host must retain the secret itself purely for
   that. `Connection::config()` or `start_scram(password, binding)` would fix
   it. The most awkward thing this lane hit.
2. **No owned form of `Event` / `Row` / `Fields`.** `types::Value::into_owned`
   exists, but `Row` yields `Result<Option<&[u8]>>` and `Fields` yields
   `Result<Field<'a>>`, so every host writes the same ~80-line materialization
   layer. An `Event::into_owned()` or an `OwnedRow` would delete it from all of
   them.
3. **`abort(reason: Error)` has no variant for the host's own transport
   diagnostic.** `Error::Transport` displays as "Connection terminated
   unexpectedly", so the real cause (`ECONNREFUSED connect -111`) cannot flow
   through the core's own `Outcome::Aborted` completions and every host shadows
   it in its own state.
4. **`types::decode` renders an unknown OID's text as `Value::Text`.** From the
   return value alone a host cannot distinguish "decoded as text" from "no
   codec, here are the raw bytes". A host that matches on the `Value` variant
   rather than the OID will silently widen its JS type surface. A
   `Value::Unknown { oid, text }` would make the distinction visible.
8. **`next_event()` returns `Ok(None)` in `State::Scram(_)`.** A host that
   ignores `ScramNeeded` stalls silently rather than erroring.
9. **`Connection::new` queues the StartupMessage before the host has a
   transport**, so `output()` is already non-empty at construction — convenient,
   but the README's step 1/2 ordering does not mention it.

**`turnloop-mysql`**

10. **`COM_STMT_CLOSE` and `COM_QUIT` cannot complete under a completion-driven
   host.** `next_event()` performs `NoResponse → Completed` and
   `Closing → Closed` only on a *subsequent* call, after `output()` is
   acknowledged — but neither command produces a server reply, so nothing wakes
   the connection and the queue wedges. Worked around here with a 0 ms turnloop
   deadline. A `consume_output` return value, or a `poll_pending() -> bool`
   saying "call me again", would remove the workaround. **Highest-value fix.**
11. **`Event::Ok` is overloaded**: it fires for a real OK packet (carrying
   `affected_rows`/`last_insert_id`) and for the EOF terminating a result set,
   where those fields are not row counts. The host must track whether a result
   set is open to tell them apart.
9. **`accept()` also requires `output().is_empty()`**, coupling submission to
   host flush timing. A public `can_accept() -> bool` would let an adapter's
   queue guard be exact instead of a hand-copy of three private preconditions.
10. **`Row::parse` validates by cloning the row and iterating it fully**, then
    the host iterates again — every row is decoded twice.
11. **`close_statement` removes the statement from `self.statements` before its
    bytes are acknowledged**, so a close that fails to flush leaves the core
    believing a statement is gone while the server still holds it.
12. **`types::decode` cannot express the `mysql2` policy Perry ships** — no
    option truncates a DATETIME's sub-second part, and because MySQL reports
    `character_set == 63` for every non-string type, TIME, BIT and GEOMETRY all
    fall into the `Buffer` arm. This binding ended up not using `decode` at all.

**`turnloop-mongodb`**

13. **`CursorBatch::rows()` over-captures under Rust 2024.**
    `pub fn rows(&self) -> impl Iterator<Item = Result<&'a RawDocument>>`
    captures `&self` as well as `'a`, so `match batch.rows().next() { … }` in
    tail position fails to compile with E0597. rustc itself suggests
    `+ use<'a>`. A one-line fix that will otherwise bite every host.
14. **No way to ask the connection what state it is in.** `receive()` errors if
    the connection is not expecting a reply and only `is_ready()` is exposed, so
    every host shadows the state machine with its own `expecting_reply` flag. An
    `accepts_receive()` would remove a class of host bugs.
15. **Two separate host-entropy obligations, documented in two places.** The
    SCRAM nonce is in the README; `ObjectIdGenerator` lives in `command` and is
    easy to miss until an `insertOne` reaches the server without `_id`.
16. **`WriteResult::parse` only reports failure when `ok == 0`.** A duplicate key
    answers `ok: 1` with a `writeErrors` array, so a host that trusts
    `WriteResult::parse` alone silently resolves a failed insert. The safe order
    (`Error::from_response` first) is not stated anywhere.
17. **`Connection::fail()` while a reply is unreleased pushes no `Failed`
    event**, so the token silently vanishes and a host cannot rely on events
    alone to settle everything.

**Shared**

18. **`LocalExecutor` silently drops completions it did not issue** — P5's
    finding, unchanged, and the reason all four `asynchronous` modules are
    unusable here.
19. **No socket-option API** — P1's finding. `turnloop_net` sets `TCP_NODELAY`
    at creation, which covers these drivers, but a host that wanted to change it
    later still cannot.

## Perry defects this work found (not P7 regressions)

Each reproduced on the base commit, so each is pre-existing and wants its own
issue rather than being folded into this change.

1. **`client.query(sql, params)` never reaches the server with its parameters
   in `pg`.** Both transports answer `bind message supplies 0 parameters, but
   prepared statement … requires 1`. The whole parameterized-query surface of
   Perry's `pg` binding is dead, and nothing in the repository tested it — there
   is no `test-files/` fixture mentioning `pg` at all.
2. **`pg`'s result object is unreadable from TypeScript on the sqlx path.**
   `res.command`, `res.rowCount` and `res.rows` all read back `undefined` on the
   base commit. *Fixed for clients that take the turnloop path.*
3. **`find().toArray()` resolves an empty string in `mongodb`.** MongoDB's
   primary read API returns nothing usable, on both transports —
   `insertOne().acknowledged` is `false` and `updateOne().modifiedCount`,
   `deleteOne().deletedCount` and `insertMany().insertedCount` are `undefined`
   as well. The wire half works; the JS half does not.
4. **Seven `js_ioredis_*` entry points are unreachable from TypeScript.**
   `setex`, `ping`, `hget`, `hset`, `hdel`, `hlen` and `hgetall` exist as
   `#[no_mangle]` symbols in *both* the stdlib and the ext binding, and the
   compiler's native-method table
   (`crates/perry-codegen/src/lower_call/native_table/databases.rs`) has no row
   for any of them — so the call falls through and returns `undefined`.
   Reproduced identically on base and on this branch:
   `setex: undefined … ping: undefined … hgetall → TypeError: Cannot convert
   undefined or null to object`. The whole Redis hash family is dead from JS.
5. **A method call whose receiver is an array element does not reach the native
   table.** `clients[0].set(…)` and `clients.map(c => c.get(…))` return
   `undefined` where `c0.set(…)` works, on both commits. Same class as
   `test_issue_536_user_pool_class`.
6. **`new Redis()` defaults to TLS, which no Perry build can serve.**
   `REDIS_TLS` unset means `true`, which builds a `rediss://` URL, and the
   `redis` dependency has no TLS backend compiled in. The default constructor
   cannot connect; every working program must set `REDIS_TLS=false`.
7. **`js_ioredis_hgetall` built its result object on a tokio blocking-pool
   thread** — `alloc_string` / `js_object_alloc_with_shape` inside the
   `spawn_blocking` closure, #1824's exact shape, in the binding that is live by
   default. The stdlib copy does it correctly. *Fixed for clients that take the
   turnloop path; the legacy path still has it.*
8. **The sqlx `pg` path does the same.** `rows_to_pg_result` calls
   `alloc_string`, `js_array_alloc` and `js_object_alloc_with_shape` inside the
   `spawn_blocking` closure in `js_pg_client_query`, `js_pg_client_query_params`
   and `js_pg_pool_query`. Same fix, same remaining exposure.
9. **`get_handle_mut::<PgConnectionHandle>` hands out a `&'static mut` from a
   blocking-pool thread while the main thread can `take_handle` the same
   handle** (`js_pg_client_end` does exactly that). A pre-existing aliasing
   hazard in the legacy path.
10. **`perry-ext-pg` and `perry-ext-mysql2` use plain `spawn_blocking`, not
   `spawn_blocking_with_reactor`, for real socket I/O** — the exact shape
   `perry_ffi_async.rs:252-278` says panics with "there is no reactor running",
   and the reason that second shim was added for net/ws/http. It evidently works
   because `binding_needs_shared_tokio` forces a shared tokio compilation, but
   nothing states that this is what makes it safe.
11. **`const { Client } = pg` does not work.** Destructuring a native module's
    default export gives `undefined is not a constructor`; `import { Client }
    from "pg"` does. Pre-existing and unrelated to the transport, but it is what
    the documented snippet in `docs/examples/stdlib/database/snippets.ts` would
    hit if it were ever run.
12. **A database handle is process-global while the turnloop connection is
   thread-local**, so a handle created on the main agent and used from a
   `perry/thread` worker now rejects where sqlx would have worked. This is a
   consequence of the migration rather than a pre-existing defect; it applies to
   all four bindings and wants one tracker covering them.

## What P7 did not do

Named precisely, because each is a hole rather than a preference.

* **TLS to any database**, and therefore SCRAM-SHA-256-PLUS channel binding. See
  the TLS section: nothing regresses, but nothing improves either.
* **Compression on MySQL and MongoDB.** The alpha.4 fix is adopted; the feature
  is off because Perry exposes no option for it.
* **Streaming results / cursors as a JS surface.** MongoDB's `find` follows its
  cursor, but the result still materializes in one go, as it did before.
* **`turnloop_postgres::pool` and `turnloop_mysql::pool`.** Both want host-executed
  Connect/Close events that `perry_db_turnloop` does not expose; the pools here
  are the driver's own.
* **`perry-stdlib`'s copies of all four modules.** They are compiled out of any
  default build by the well-known flip and are only reachable under
  `PERRY_DISABLE_WELL_KNOWN=1`. Untouched, like P1 left the bundled stdlib `net`.
* **`bun:sql`, `better-sqlite3`, `node:sqlite`.** SQLite is not a network driver
  and has no event-loop transport to move.
* **A Windows or macOS arm.** Everything measured here ran on Linux x86_64. The
  protocol cores are portable Rust over P1's socket layer, and neither has been
  exercised elsewhere.
* **Any benchmark.** The build box is shared and was running other lanes' work
  throughout; DESIGN §12's per-phase instruction A/B at cgu=1 with a control
  probe has not been taken.
* **A saturation soak.** `mysql2`'s pool is bounded at ten and queues beyond
  that. `mysql_pool_parity.ts` exercises acquire, pinning, isolation and release
  against a real server, but never with more than ten callers outstanding, so
  the **queue** and the acquire deadline are still reviewed rather than
  exercised.

## For the integrator

The branch is `turnloop/p7-databases` on `origin`. Nothing here bumps the
version, and there is a `changelog.d/` fragment.

To reproduce, on a machine with the pinned oracle and the four servers:

```bash
# unit tests (perry-runtime's and perry-ext-mysql2's are not parallel-safe)
CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo test -p perry-db-turnloop
RUST_TEST_THREADS=1 cargo test --release -p perry-ext-mysql2
cargo test --release -p perry-ext-ioredis -p perry-ext-pg -p perry-ext-mongodb
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime turnloop_net

# the gap suite, against a baseline built from this branch's OWN base commit
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh

# the acceptance fixtures (see the server table above for the environment)
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/redis_parity.ts
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/pg_parity.ts
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/mysql_parity.ts
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/mysql_pool_parity.ts
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/mongo_parity.ts
/root/claude-turnloop-p7/p7run.sh <tree> scripts/turnloop/apps/pg_thread_census.ts

# the headline measurement, on both arms
PERRY_LOOP_STATS=1 ./db_thread_census
PERRY_LOOP_STATS=1 ./pg_thread_census

# GC stress with replies in flight, several seeds
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
  PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 ./redis_gc_stress
```

Still to run, and **not** run here: a Windows arm, a macOS arm, the
auto-optimize gap tier, `cargo test --workspace`, an instruction A/B, and a
saturated-pool soak.

The two trees are on the build box at `/root/claude-turnloop-p7/{base,perry}`
(base at `7f77cce3c6`), each with its own `target/`, plus `oracle/` and
`dbservers.sh`. Delete them when the A/B is done, and stop the servers with
`dbservers.sh stop`. `PERRY_RUNTIME_DIR` must be overridden per tree —
`/etc/profile.d/perry.sh` points it at a different checkout.
