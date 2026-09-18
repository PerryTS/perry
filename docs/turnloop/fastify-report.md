# turnloop — `perry-ext-fastify` and the bundled framework server

> **Partly superseded, 2026-09-17.** This report's group-F conclusion — that
> fastify's four edges survive because `perry-ext-ws` needs an owned
> `AsyncRead + AsyncWrite` stream — was correct when written and is no longer
> true. The group E/F lane removed `perry-ext-ws`'s tokio transport, which
> removed the blocker, and **group F's edges and group E's are all gone**:
> `perry-ext-fastify` declares no hyper, hyper-util or tokio, and the hyper
> accept loop is deleted. Every statement below about group F surviving, about
> the listen-time decline, and about `perry-ext-ws` being untouched should be
> read as history. See "The one declining case, and how it was closed".

Branch `turnloop/fastify`, based on `turnloop/integration` at `0f0a4d6b6f`.
Built and tested on the shared Linux box (EPYC 9354P) against the pinned gap
oracle Node **26.5.1**. Nothing here was run on Windows, and nothing was
benchmarked.

Two of `scripts/tokio_inventory.py`'s groups:

* **F** — `perry-ext-fastify`'s four edges (`hyper`, `hyper-util`, `tokio`,
  `tokio-tungstenite`), whose recorded blocker is *"its own hyper accept loop,
  independent of perry-ext-http's … sharing that core needs either a new crate
  for the sans-I/O server or a new edge."*
* **H** — `perry-stdlib`'s six optional edges (`hyper`, `hyper-util`,
  `mongodb`, `redis`, `sqlx`, `tokio-rustls`), whose `hyper` blocker is
  *"`framework/server.rs` is a hyper service Perry never migrated."*

## The design question, answered

> extract P5's sans-I/O server into a crate both can depend on, or give
> `perry-ext-fastify` an edge to `perry-ext-http`?

**Extraction.** `crates/perry-http-server` is the new crate: the HTTP/1.1
server core on turnloop — one multishot `accept_start`, one multishot
`read_start`, the `turnloop_http::http1` codec driven sans-I/O, Node's framing
rules, its `Connection`/`Keep-Alive` decision and its idle-close arithmetic —
behind a `Host` trait. `perry-ext-fastify` and `perry-stdlib`'s
`framework/server.rs` both sit on it.

The decisive argument is not the abstract one about coupling. It is that **the
edge does not work, and would look like it did.**

1. **It would put hyper back, transitively, and turn the gate green anyway.**
   `perry-ext-http` still needs hyper after P5 — a `worker_threads` agent, a
   cluster worker, an attached `WebSocketServer`, and `reqwest` — so
   `perry-ext-fastify → perry-ext-http → hyper` is a live edge, not a
   formality. `scripts/tokio_inventory.py` gates *manifest* edges, so that swap
   would delete four lines from `scripts/tokio_inventory.json` while every
   fastify program still linked hyper. Removing an edge from the ratchet
   without removing it from the binary is the failure mode the ratchet exists
   to prevent.
2. **Both crates are `staticlib`s, so the edge is a bundling decision too.**
   `libperry_ext_fastify.a` would carry every object of `perry-ext-http` —
   hyper, h2, reqwest, rustls, webpki-roots — into every program that imports
   `fastify` and nothing else.
3. **It does not help group H at all.** `perry-stdlib`'s framework server is
   the *fallback* the well-known flip replaces with `perry-ext-http`; an edge
   from the fallback to the wrapper inverts the layering, and drags hyper back
   into `perry-stdlib` by the same transitive route. So the edge solves one of
   the two groups, badly, and the extraction solves both.

That third point is also why this is not a speculative abstraction: the crate
has **two consumers on the day it lands**, not one.

### What was given up

* **`perry-ext-http` did not move onto it.** Its `server/turnloop_serve` is the
  most heavily verified surface in this area — byte-identical gap tests against
  Node, and `turnloop_h2` beside it at h2spec 147/147 — and rewriting it to sit
  behind a trait in the same change that migrates two other servers would put
  that at risk for no edge. So **there are two HTTP/1.1 server state machines
  in the tree until it does**, and they can drift. Three things bound that: the
  crate's API was shaped from `perry-ext-http`'s actual call sites — the
  streaming trio (`stream_begin` / `stream_body` / `stream_end`), the interim
  `100 Continue` path and the `'aborted'` callback are all here and neither
  consumer uses any of them — so that migration is a re-point rather than a
  rewrite; the Node-fidelity rules that took P5 four fixes to find live in the
  crate **with their tests**; and `turnloop_serve`'s own header now names it.
  What was deliberately *not* pre-built for it is TLS — see below.
* **A TLS seam, on purpose.** An earlier draft of the crate carried a
  `TlsLayer` trait — six methods and a branch on every read, write, close and
  terminal completion — so that `perry-ext-http`'s migration would find one
  ready. **No consumer implements it**, so every one of those branches was a
  configuration nobody had run. CLAUDE.md's rule for exactly that shape ("a
  mode that still exists is a decision that hasn't been made… the losing mode
  should stop compiling, not linger as an untested configuration that a future
  bisect will trust") is written about GC knobs and applies just as well to a
  server core, so it was deleted before this landed. The migration that needs
  TLS adds the seam against its real caller, which is a better design than a
  guess nothing exercises. This is the one place the "shaped from the real call
  sites" argument above does *not* apply, and it is a smaller migration for it.
* **HTTPS and HTTP/2.** The crate serves cleartext HTTP/1.1. Neither consumer
  needs more: `perry-ext-fastify` has no `https` option at all, and although it
  declares hyper's `http2` feature it has only ever built an `http1::Builder`
  — the brief's premise that "fastify serves both" does not hold for this
  binding, and `lib.rs`'s own "Punted gaps" section says so.

## What moved, and what did not

| surface | transport after this change | why |
|---|---|---|
| `fastify()` + `app.listen({ port })` | **turnloop** | — |
| routing, path params, JSON bodies, `reply.code`/`send`, hooks, error handler | **turnloop** | — |
| a `404` on an unmatched route | **turnloop**, answered in the sink | the hyper service fn answered it without a main-thread hop either |
| HEAD shadowing a GET route | **turnloop** | the core frames a HEAD response body-forbidden from the *real* request method |
| `{ reusePort: true }` and a `cluster.fork()` worker | **turnloop** | `turnloop_net::tcp_listen` takes `reuse_port`; fastify's cluster path is SO_REUSEPORT only, no fd passing |
| a fastify app with `app.server.on('upgrade', …)` handlers | **turnloop** (was hyper) | closed by the group E/F lane — see below |
| the bundled `js_http_server_*` framework server | **turnloop** | — |

### The one declining case, and how it was closed

> **Superseded 2026-09-17 by the group E/F lane.** The section below is kept
> because the diagnosis was right and the fix followed it exactly; what changed
> is the premise's second half.

`app.server.on('upgrade', …)` used to end in
`perry_ext_ws::register_external_ws_stream`, whose signature was
`<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(WebSocketStream<S>)`. A
turnloop connection cannot produce such a stream. That much still holds. What
this report got wrong was "`perry-ext-ws` has no turnloop path at all": by the
time it was read, the *protocol* had already moved to `turnloop_websocket`'s
sans-I/O codec and only the transport was left on tokio.

So the fix was neither of the two the report proposed. Not a descriptor
handoff — `turnloop::Driver::detach` is still not exposed through
`perry_ffi::turnloop_net`, and did not need to be. Not "`perry-ext-ws` moving
to `turnloop-websocket`" either — it was already there. It was the *transport*:

* `perry-ext-ws` lost its tokio driver outright (a turnloop `tcp_connect` plus
  a `perry_tls_session` layer for `wss://`, and `perry-http-server` for the
  standalone `WebSocketServer({port})`);
* `perry-http-server` grew the upgrade hook its own module header had recorded
  as withheld "until that is solved, with a caller"
  (`Host::takes_upgrades` / `on_upgrade` / `on_upgraded`);
* `FastifyHost` implements it, answering the handshake with
  `perry_ext_ws::accept_http_upgrade` on the connection the core already owns.

**Group F's four edges are gone, and so is group E's.** `perry-ext-fastify`
declares no `hyper`, `hyper-util`, `http-body-util`, `bytes`, `socket2` or
`tokio`; the whole hyper accept loop, its service fn and its upgrade task are
deleted. The listen-time decline is deleted with them: an agent with no
`turnloop::Loop` now reports that through `listen()`'s `(err, address)`
callback instead of falling back to a second transport.

## Group H: four of its six edges are not this lane's

The inventory groups `perry-stdlib`'s six optional tokio-family edges together,
but they are three unrelated subjects, and only one is the framework server:

| edge | what actually pulls it | this lane |
|---|---|---|
| `hyper`, `hyper-util` | `framework/server.rs`, behind the `http-server` feature — which `full` enables, so **every default build of `perry-stdlib` carried them** | **removed** |
| `sqlx`, `redis`, `mongodb` | the *bundled database drivers* (`bundled-pg`, `bundled-mysql2`, `bundled-ioredis`, `bundled-mongodb`). Nothing in `framework/` references them; they are P7's subject and are reached through the stdlib's data layer, not its server | not touched |
| `tokio-rustls` | `src/tls.rs` (the bundled TLS server / `node:tls`) and `src/net/mod.rs` (bundled net client TLS), behind `tls-runtime` — which `external-net-tls` and `external-tls-server` also pull in, so it survives the well-known flip | not touched |

This is the overlap the brief asked about, and the answer is yes: **the three
database edges and `tokio-rustls` belong to the lane giving the database
drivers TLS, not to the fastify/server lane.** Attributing them here would have
claimed four edges this change cannot remove.

## The two traps, and what was done about them

1. **`http1::Decoder` raises `Event::End` from a step that consumes zero
   bytes** (PerryTS/turnloop#50). The core's decode loop therefore continues on
   "an event, **or** bytes consumed", and only `None` with `consumed == 0` ends
   it. The rule is written out in `conn.rs`'s `drain` doc comment, with the
   failure it prevents (a host that loops on `consumed > 0` never sees `End`,
   never dispatches, and stalls with the client waiting) named in the same
   place, because the next lane to write one of these will read that function
   and nothing else.
2. **`noDelay` in `reuse_port`'s argument slot.** `perry_http_server::listen`
   takes both as named arguments with the history in the doc comment, and
   **both consumers pass them explicitly**: fastify passes `reuse_port` from
   `{ reusePort: true }` or cluster-worker detection and `no_delay: true`
   (Node's `http.createServer` default), and the bundled framework server
   passes `false, true` with a comment saying which is which.

A third, not in the brief but paid for the same way: a host that answers
*inside* `Host::on_request` — fastify's 404 does — re-enters `decode` through
`complete_response`, so the stack depth would be the client's to choose via
pipelining. The core guards it with a drain flag rather than recursing.

## Evidence

Every command as run, on the shared Linux box, against Node **26.5.1**
(`/opt/node-v26.5.1-linux-x64`, the pinned gap oracle). Nothing was
benchmarked.

### The acceptance fixture

`scripts/run_fastify_tests.sh` — the repository's own fastify end-to-end
harness (routing, path params, a JSON body, `reply.code`, a 404, a sync throw,
an async rejection, `setErrorHandler` with an `instanceof`-narrowed error).
Run on **both** arms, reading the harness's own exit code rather than a
wrapper's:

| | base `33f61fc9e7` | this branch |
|---|---|---|
| `harness exit` | 0 | 0 |
| checks | **12 passed, 0 failed** | **12 passed, 0 failed** |

### The wire, against real fastify on the oracle

`test_fastify_integration.ts` cannot run under `node
--experimental-strip-types` (it uses a TypeScript parameter property, which is
why the file is `parity-skip`), so the oracle is `fastify@5` on Node 26.5.1
serving the same four routes from plain `.mjs`. Header **names, values and
order**, as `curl -i` sees them:

| | Node 26.5.1 + fastify | base (hyper) | this branch |
|---|---|---|---|
| `Date` | `Date: …` | `date: …` *(hyper's lowercase)* | **`Date: …`** |
| `Connection` | `Connection: keep-alive` | *(absent)* | **`Connection: keep-alive`** |
| `Keep-Alive` | `Keep-Alive: timeout=72` | *(absent)* | **`Keep-Alive: timeout=72`** |
| header order | ct, cl, Date, Connection, Keep-Alive | ct, cl, date | **ct, cl, Date, Connection, Keep-Alive** |
| an HTTP/1.0 request | `HTTP/1.1 200 OK` + `Connection: close` | `HTTP/1.0 200 OK`, no `Connection` | **`HTTP/1.1 200 OK` + `Connection: close`** |
| connection reuse | `200 1` then `200 0` | same | same |
| three pipelined requests, one write | 3 answered, socket kept open | same | **same** |
| a control byte in a header value | 400 | 400 | 400 |
| HEAD shadowing a GET route | head only, `content-length` kept | same | same |

`timeout=72` is fastify's own `keepAliveTimeout` default, **measured** against
the oracle rather than read from a doc — Node's server default is 5 s, and
taking that would have produced `timeout=5` against every real fastify client.
The hyper path advertised neither header and armed no idle close at all, so
this is the first time a fastify server on Perry has one.

**Two differences remain, and neither is this change's.** They are identical
on both Perry arms and predate the migration: `content-type:
application/json` where Node sends `application/json; charset=utf-8`, and a
404 body of `{"error":"Not Found"}` where Node's fastify sends
`{"message":"Route GET:/nope not found","error":"Not Found","statusCode":404}`.
Both are the binding's response envelope rather than its transport. Worth
their own issue; folding them in here would have made the transport A/B
unreadable.

### `PERRY_LOOP_STATS`, and what the discriminating quantity actually is

One fastify server, **20 in-process requests**, clean `app.close()` so the
exit reporter runs. Both arms, same fixture, same binary layout:

| | base (hyper) | this branch |
|---|---|---|
| `driver` | turnloop | turnloop |
| `turns` | 61 | **101** |
| `completions` | 79 | **166** |
| `native_ticks` (tokio ticks inside the park) | **39** | **0** |
| `tokio_ticks` | **39** | **0** |
| `tokio_tick_ns` | **39,060,095** | **0** |
| `turnloop_waits` | **0** | **59** |
| requests answered | 20 | 20 |
| threads at start / peak while serving / after close | 1 / 1 / 1 | 1 / 1 / 1 |

**The thread count does not move, and reporting it as the headline would have
been wrong.** Perry drives tokio as a *current-thread* runtime from inside
`js_wait_for_event`'s wait driver, so a hyper fastify server never had a worker
thread to lose — it had 39 ms of tokio ticks inside the park instead. The
discriminating quantity is `tokio_ticks` / `native_ticks` going to zero while
`turnloop_waits` goes from 0 to 59: nothing in the process holds a tokio task
any more. The thread count is reported because it was asked for, and it is
reported as *unchanged* rather than dressed up.

### Unit tests

| suite | result |
|---|---|
| `cargo test -p perry-http-server` | 15 passed |
| `cargo test -p perry-ext-fastify` | 32 passed |
| `cargo test -p perry-stdlib framework::` | see the run below |

`perry-http-server`'s 15 are the framing rules that took P5 four Node-fidelity
fixes to find, now testable without a server: a custom reason phrase, a
close-delimited HTTP/1.0 body, a HEAD response that keeps its advertised length
and sends no body, a 204 that synthesizes none, `Transfer-Encoding` in Node's
casing, and the six-row `Connection`/`Keep-Alive` matrix including the
`keepAliveTimeout = 0` row that means "no timeout", not "no keep-alive".

`perry-ext-fastify`'s deferred-queue tests changed shape with the transport and
are stronger for it: the backpressure test used to assert that an over-cap
pending's `oneshot` was *closed*; it now asserts that the client is **answered
503**, which is what `FastifyPendingRequest::drop` does on both transports.

### The tokio inventory

```
$ python3 scripts/tokio_inventory.py
tokio inventory: 36 manifest edges across 12 workspace crates,
20 tokio-family packages in Cargo.lock — unchanged.
```

38 → **36**, and the two that went are `perry-stdlib`'s `hyper` and
`hyper-util`. Group H drops 6 → 4, group F stays at 4.

### The gap suite, both arms

<!-- GAP -->

### The other gates

| gate | result |
|---|---|
| `scripts/check_file_size.sh` | OK (the fastify listen path moved to `listen.rs` to stay under the 2000-line cap) |
| `scripts/addr_class_inventory.py` | OK |
| `scripts/raw_handle_debt.py` | 901 sites (baseline 901) |
| `scripts/check_node_version_consistency.py` | OK |
| `scripts/gc_runtime_root_holders.py` | OK — see below |
| `cargo fmt --all -- --check` | clean |

**`gc_runtime_root_holders` needed two edits, and one of them is worth
reading.** The new one is honest debt: `perry-ext-fastify`'s `DEFERRED` queue
became visible to rule S when its pending's response channel turned into a
`Reply` enum, so it now carries a written verdict (it holds registry ids and
Rust-owned bytes; the handler closures it dispatches to live in the
`FastifyApp` handle the registered scanner visits).

The other is a false positive the gate cannot distinguish: `MESSAGE_IDS` in
`perry-stdlib/src/nodemailer/turnloop_bridge.rs` — a holder this change never
touches — flipped to *covered*, which makes its inventory entry stale and
which the gate requires be deleted. It flipped because coverage is a
deliberately over-approximating bare-name reachability walk (depth 3, any
identifier), and this change's new function bodies in `perry-ext-fastify`
extend that walk until it reaches a function in that file. Bisected to the
`perry-ext-fastify` files (reverting only them restores `covered: false`); no
single identifier is responsible, so there is nothing to rename. The entry is
deleted as the gate demands, and the substance is not lost: the holder's own
doc comment already says what the verdict said — a `HashMap<usize, String>`
keyed by a pinned promise address, no JS value, nothing for a moving collector
to invalidate. Flagged here because "an entry goes stale when the holder
becomes COVERED, which is exactly what a fix looks like" is **not** what
happened, and a reader of the diff deserves to know which one it was.

## A defect in the template this found

`perry-ext-http`'s `turnloop_serve` has **no `NET_SHUTDOWN` arm**. Its
`finish_and_close` submits `tl::shutdown(id, 0)` and then waits for a close
that nothing ever asks for: the completion it needs is delivered
(`turnloop_net` emits `NetCompletion::shutdown`, and `perry-ext-net`'s sink
answers it with `close_after_shutdown -> destroy`), but `turnloop_serve`'s
`match` has no arm for it and falls through `_ => {}`. Its `on_eof` then
returns early because `closing` is already set, so the terminal `Closed` never
arrives and `free_handle_id` never runs.

That is one runtime entry and one handle id per connection the **server** ends
— `Connection: close`, an HTTP/1.0 response, a 400 on a malformed request, the
idle keep-alive deadline, `server.close()` — which is the #6441 exhaustion
shape `on_closed`'s own comment says it exists to avoid. It bites exactly the
paths P5's own tests exercise least, because a keep-alive connection the
*client* closes does reach `Closed`.

This crate has the arm (`conn::on_shutdown`), with the reasoning written beside
it. `perry-ext-http`'s copy is **not** fixed here: it is a one-line change in
another lane's file, and it deserves its own before/after rather than riding in
on a fastify commit. Reported so the P5 lane can take it.

## What this did not do

* **`perry-ext-http` did not migrate onto the crate.** See "What was given up".
* **`perry-ext-ws` is untouched**, which is what keeps fastify's four edges
  alive. Two things would close it: `perry_ffi::turnloop_net::detach` (turnloop
  has `Driver::detach` + `Detached::into_fd`; nothing exposes it to a binding),
  or `perry-ext-ws` on `turnloop-websocket` — which P5 already noted needs the
  tungstenite major to change with it (0.29 vs 0.30).
* **No HTTPS and no HTTP/2 in the new crate.** Neither consumer serves either.
* **No Windows arm.** The codec and the framing are platform-independent; the
  accept path and the error table are not.
