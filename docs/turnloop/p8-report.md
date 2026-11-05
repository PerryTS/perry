# turnloop P8 — taking tokio out: the inventory, and what it would cost

Branch `turnloop/p8-detokio`, based on `turnloop/integration` at `babc5f0d1f`
(P0–P7 merged, plus `main` through v0.5.1576). Built and tested on the shared
Linux build box (`perrybuilder`, EPYC 32c/64t) against the pinned gap oracle
Node **26.5.1** (`/opt/node-v26.5.1-linux-x64/bin`, not the box default
26.8.1). Nothing here ran on Windows or macOS, and nothing here was
benchmarked — see "What P8 did not do".

## Epilogue: tokio is gone (final tokio lane)

This report is a historical record of P8's starting point. Everything below
describes the tree at `babc5f0d1f`, not the current one. The costed removal
plan it laid out (groups A–N, "What it would cost to actually remove tokio")
has since been carried out lane by lane, and the final tokio lane deleted the
last edge — group **L**, perry-stdlib's own `tokio`:

* `perry-stdlib`: `common/tokio_bridge.rs`, the `async-runtime` feature (and
  its place in `full`) and `dep:tokio` are deleted. `common::async_bridge`
  (the `async-bridge` feature) is the only async bridge; the bundled sharp
  module's three async ops moved onto turnloop's pool
  (`pool_for_promise_deferred`), which also moved their JS string
  construction onto the main thread.
* `perry-ffi`: `spawn_async` / `spawn_blocking_with_reactor` and their C
  symbols are **retired** (no caller was left; see
  `docs/src/native-libraries/abi.md`, "Retired"). `spawn_blocking` has one arm,
  on turnloop's long-occupancy set; `run_pending` is one bounded loop turn.
* CLI: the #7629 shared-tokio link check (`compile/shared_tokio.rs`),
  `binding_bundles_tokio`, the driver's `async-runtime` selection and the
  no-auto tokio warning are deleted. The #507 co-build set stays, renamed
  `binding_cobuilds_with_stdlib` (cache-key segment `cobuild=`); it no longer
  exists for tokio's sake.
* Workspace: `tokio`, `tokio-tungstenite`, `tokio-rustls`, `reqwest`, `hyper`
  and `hyper-util` are gone from `[workspace.dependencies]`; `tokio`,
  `tokio-macros` and `mio` 1.x left `Cargo.lock`.
  `cargo tree --workspace --all-features --target all -e normal,dev,build -i tokio`
  matches no package.
* `scripts/tokio_inventory.py` is no longer a ratchet but a **ban**: any tokio
  manifest edge (normal, dev or build, any target), any tokio package in
  `Cargo.lock` or any non-comment `tokio::` in `crates/*` fails `lint`, with no
  allowlist. `deny.toml` bans the same crates for cargo-deny. The two crates the
  old inventory tracked as "tokio family" that are not tokio — `tungstenite`
  (synchronous; perry-ui-android and turnloop-websocket's codec) and `lettre`
  (message builder only) — are listed as `not_tokio`, and the gate verifies
  from `Cargo.lock` that neither depends on tokio.

The #10671 hazard (two archives from separate cargo invocations bundling two
tokio compilations) cannot occur any more: there is no tokio to duplicate.

## The verdict, first (at P8)

**tokio cannot leave Perry's dependency graph in this lane, and it is not
close.** `Cargo.lock` holds 20 tokio-family packages and the workspace holds
**46 manifest edges** to them across **16 crates**. Not one of those edges is
removable by a transport swap: every one is either a surface no lane has
migrated at all, or the still-reachable fallback of a surface that was
migrated only for the primary agent.

The gap suite is clean — 807 tests on both arms, **zero Perry-side status
changes**, the same nine known failures — but that is not the achievement,
because this branch changes almost no runtime code. The achievement is the
inventory, and a defect the inventory's probe found.

The count is unchanged by this branch. Before and after, on
`turnloop/integration` @ `babc5f0d1f` and on `turnloop/p8-detokio`:

| `grep -c '^name = "X"' Cargo.lock` | before | after |
|---|---|---|
| `tokio` | 1 | 1 |
| `hyper` / `hyper-util` / `hyper-rustls` | 1 / 1 / 1 | 1 / 1 / 1 |
| `h2` | 1 | 1 |
| `reqwest` | 1 | 1 |
| `lettre` | 1 | 1 |
| `sqlx` (+ `-core`, `-mysql`, `-postgres`) | 1 (+3) | 1 (+3) |
| `redis` | 1 | 1 |
| `mongodb` | 1 | 1 |
| `tokio-rustls` / `-tungstenite` / `-util` / `-stream` | 1 each | 1 each |
| `tungstenite` | 2 (0.24 and 0.29) | 2 |
| `tower` / `tower-http` | 1 / 1 | 1 / 1 |

```
$ python3 scripts/tokio_inventory.py
tokio inventory: 46 manifest edges across 16 workspace crates,
20 tokio-family packages in Cargo.lock — unchanged.
```

### One thing that must not wait for the inventory

Building the probe that measures the "a worker agent declines to tokio" claim
every lane depends on turned up a **regression on this integration branch**:
`fetch()` inside a `node:worker_threads` Worker answers `status=200` on `main`
and `error: fetch failed` on `turnloop/integration`. Deterministic, three runs
on each arm, both built from source here. Full evidence and cause in "Perry
defects this work found" below. It should be fixed before this branch merges,
and it is the reason this report leads with it rather than with the table.

### The rest of the lane

So this lane's deliverable is the inventory, and the inventory is a gate rather
than a paragraph. What it says, in one sentence: **one missing capability — a
`turnloop::Loop` on an agent that is not the primary one — is why every surface
P1, P5, P6 and P7 migrated still carries its tokio transport**, and five
further surfaces (the `node:http` client, HTTP/2, `fastify`, `ws`, and both npm
HTTP-client wrappers) were never migrated at all.

## Why the inventory is a script and not a section

P0–P7 each ended with a prose list of what it did not move. Those eight lists
are the only record of the remaining surface, and reading all eight against the
tree turned up three problems, none of which is a criticism of any individual
report:

* **They are incomplete, and nothing could tell you.** No lane report names the
  `perry` CLI, `perry-container-compose` or `perry-ui-gtk4`. Between them those
  hold **6 of the 46 edges** — a seventh of the problem, invisible because no
  lane's scope included a crate outside the default runtime build (the CLI and
  gtk4 have no JS surface; perry-container-compose is reached only through
  perry-stdlib's non-default `container` feature), and no lane was measuring
  edges in the first place.
* **They scope the same blocker differently each time.** Every lane from P1 on
  says some version of "a worker agent has no loop". None says that it is *one
  predicate in one function* (`agent_loop::net_available`) that gates net, TLS,
  the HTTP server, fetch, SMTP and all four database drivers simultaneously —
  and, as this lane measured, none of them is right about
  `node:worker_threads`, where the predicate returns the *wrong* answer and the
  fallback is never taken at all.
* **They have no number in them.** "What did not move" is a list of names, and
  a list of names cannot be compared between two commits. A count can.

A migration whose remaining surface is measured by reading eight reports
written at eight different commits cannot be finished, because nobody can say
when it is done. So the measurement is now
[`scripts/tokio_inventory.py`](../../scripts/tokio_inventory.py), and it runs
in the required `lint` job.

### What it gates

* every **(workspace crate → tokio-family crate) manifest edge**, and
* every **tokio-family package in `Cargo.lock`**,

compared strictly against `scripts/tokio_inventory.json` **in both
directions**. A new edge fails, so tokio cannot creep back in behind a green
build. A *stale* entry fails too — so a lane that removes an edge must delete
its own line, and the file can never describe a tree that is gone. That is the
rule `scripts/gc_root_dominance_allowlist.json` already follows, and it is the
rule that makes an inventory outlive the lane that wrote it.

Edges come from `cargo metadata --no-deps`, not from `cargo tree`, for two
reasons this tree demonstrates rather than hypothesises:

1. **`cargo tree -i tokio --workspace` reports crates that have no edge.** It
   names `perry-runtime`, `perry-runtime-static`, `perry-updater`,
   `perry-ext-node-forge` and `perry-ext-undici` as tokio dependents. None of
   them is: resolved on its own, **every one of those five reaches no tokio at
   all** (`cargo tree -i tokio -p perry-runtime --target all` → "did not match
   any packages"). The workspace-wide hit is feature unification through
   `timezone_provider → combine`, whose `tokio` feature the `redis` crate turns
   on. Reading it as an edge overstates the problem by five crates and would
   send a lane to "de-tokio the runtime", which has nothing to do — the core
   (`perry-runtime`, `perry-hir`, `perry-codegen`, `perry-ffi`,
   `perry-db-turnloop`) is already tokio-free.
2. **`cargo tree` cannot see a target-gated edge on the wrong host.**
   `perry-ui-gtk4`'s tokio was `cfg(target_os = "linux")`; on the macOS
   development host it was invisible. An inventory that misses it is not an
   inventory. (That edge has since been removed — see group M — but it is
   exactly the shape the gate has to be able to see.)

### What it does *not* gate, and why that is said out loud

The per-crate count of tokio-shaped **source lines** is recorded and printed,
and deliberately not gated: a doc comment naming `tokio::spawn` moves it, so a
failure would carry no information. An ungated number living in a gate file is
a number somebody will eventually trust, so the script's docstring says which
of its two numbers is load-bearing.

## The inventory

Full text — what reaches each edge from JS, when, what blocks it, and where it
is tracked — is in `scripts/tokio_inventory.json` and renders with
`python3 scripts/tokio_inventory.py --table`. The summary:

| crate | tokio-family deps | reached from JS by | status |
|---|---|---|---|
| `perry` | reqwest, tokio, tokio-tungstenite | **nothing** — the CLI's `publish`/`login`/`verify`/`audit`/`run --remote`/`setup`/update-check | never linked into a compiled program |
| `perry-container-compose` | ~~tokio (normal + dev)~~ | `import … from 'perry/container'` / `'perry/compose'` / `'perry/workloads'` (perry-stdlib's non-default `container` feature), and the separate `perry-compose` binary | **done (lane K)** — the engine runs on turnloop through its own `rt` module; neither edge remains |
| `perry-ext-axios` | reqwest, tokio | `import axios` (its own `js_axios_*` symbols; it does **not** take the global `fetch` with it — measured) | never migrated |
| `perry-ext-fetch` | reqwest, tokio | `import 'node-fetch'` (and the bare `fetch` alias) — and it defines the **same `js_fetch_*` symbols** perry-stdlib owns | never migrated; the overlap SIGSEGVs, see defect 1 |
| `perry-ext-fastify` | hyper, hyper-util, tokio, tokio-tungstenite | `import Fastify` | never migrated — own accept loop, no edge to perry-ext-http |
| `perry-ext-http` | h2, hyper, hyper-util, reqwest, tokio, tokio-rustls, tokio-tungstenite | `http`/`https` **client**, `http2` both halves, the server on any declining path, an attached `WebSocketServer` | server migrated for the primary agent (P5); client and HTTP/2 never |
| `perry-ext-ws` | tokio, tokio-tungstenite | `import WebSocket from 'ws'` | never migrated — tungstenite 0.29 vs turnloop-websocket 0.30 |
| `perry-ext-net` | tokio, tokio-rustls | `net`/`tls` on a declining path | migrated for the primary agent (P1/P5) |
| `perry-ext-ioredis` | redis, tokio | `new Redis()` — **including the default configuration** (#10335) | migrated for plaintext on the primary agent (P7) |
| `perry-ext-pg` / `perry-ext-mysql2` | sqlx, tokio | `pg` / `mysql2` on a declining path, any TLS, a UDS host | migrated for plaintext on the primary agent (P7) |
| `perry-ext-mongodb` | mongodb, tokio | `MongoClient` on a declining path, `+srv`, `tls=`, replica sets | migrated for direct single-host plaintext (P7) |
| `perry-ext-nodemailer` | lettre, tokio | `sendMail`/`verify` on a declining path | transport migrated (P6); the MIME builder is lettre forever |
| `perry-stdlib` | hyper, hyper-util, lettre, mongodb, redis, reqwest, sqlx, tokio, tokio-rustls, tokio-tungstenite (all optional) | the global `fetch` on a declining path, `js_fetch_stream_start`, the bundled TLS server, and every bundled module under `PERRY_DISABLE_WELL_KNOWN=1` | mixed — and `tokio` here is the last edge that can go, not the first |
| `perry-ui-gtk4` | — (was tokio, `cfg(linux)`) | `perry/ui` tray + MPRIS | **removed** — the "require tokio" reading was wrong, see group M |
| `perry-ui-android` | tungstenite (`cfg(android)`) | `perry/ui` WebSocket on Android | **not a tokio edge** — sync tungstenite 0.24 on its own thread |

## The one blocker that gates almost everything

Every lane from P1 on kept its tokio transport "because a worker agent has no
loop". Stated once, precisely, from the code rather than from the reports:

```rust
// crates/perry-runtime/src/event_pump/agent_loop.rs:459
pub(super) fn net_available() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => true,
        LoopState::Declined | LoopState::ShutDown => false,
        LoopState::Unset => crate::agent::current_agent() == crate::agent::PRIMARY_AGENT,
    }
}
```

`turnloop_net::available()` is that function. `perry-ext-net`,
`perry-ext-http`'s server, `perry-stdlib`'s fetch bridge, its SMTP bridge and
all four database drivers gate on it — directly, or through
`perry_db_turnloop`'s `enabled()`, which calls it. `ensure_loop_with` sets
`LoopState::Declined` for any thread whose agent is not `PRIMARY_AGENT` and
never revisits it.

So one missing capability — a loop per agent — is what keeps a tokio
`TcpStream`, a `reqwest::Client`, a `hyper` server, an `AsyncSmtpTransport`,
`sqlx`, the `redis` crate and the `mongodb` driver all reachable from ordinary
JavaScript. **No issue tracks it.** It is the highest-leverage item left in the
whole migration, and it is a phase (P4 costed it: reshaping `PRIMARY_ROUTE`,
the notify routing and the keep-alive accounting), not a patch. It also has a
validation cost nobody has paid: turning it on makes *every* P1/P5/P6/P7
surface start taking the turnloop path on a `perry/thread` worker for the first
time, and no lane tested any of them there.

### …and running the probe found that half of it is not even true

The census probe exists because a predicate is a claim and a measurement is
evidence. Running it produced a result the eight lane reports do not describe:

```
=== perry, turnloop/integration @ babc5f0d1f ===
idle:           threads=[wcensus x1]
primary-agent:  status=200                  threads=[wcensus x1]
worker-agent:   status=error:fetch failed   threads=[wcensus x2]
[perry-loop] p6 http_submitted=2 declined=0 completed=1 failed=1 connects=1
```

`declined=0`. The Worker's fetch **did not decline to reqwest** — it was
submitted to the turnloop engine, on a thread that cannot own the loop, and
failed. Node 26.5.1 answers `status=200` on both agents.

The reason is a second predicate nobody has written down.
`agent::enter_worker_agent()` — the only thing that ever sets `CURRENT_AGENT` —
is called from exactly three places, all in
`crates/perry-runtime/src/thread.rs`: `perry/thread`'s `spawn`, `parallelMap`
and `parallelFilter`. A `node:worker_threads` `Worker` never calls it, so its
`CURRENT_AGENT` stays `None`, and:

```rust
// crates/perry-runtime/src/agent.rs:93
pub fn current_agent() -> AgentId {
    CURRENT_AGENT.with(|slot| slot.get()).unwrap_or(PRIMARY_AGENT)
}
```

**every `worker_threads` Worker reports itself as `PRIMARY_AGENT`.**
`net_available()` therefore returns `true` on it, `turnloop_client::submit`
passes its `tl::available()` guard, `SUBMITTED` is incremented, and
`exchange::start` submits a connect to a loop the thread does not own.
`ensure_loop_with` does refuse — `PRIMARY_ROUTE` is already held by the main
thread, so the Worker is marked `LoopState::Declined` — but that happens
*after* the request was accepted, so the caller gets a failure rather than the
fallback the design intends.

The predicate is **time-dependent**, which is the sharpest way to say what is
wrong with it: `net_available()` answers from `LoopState::Unset` using agent
identity until the thread's first park, and from `LoopState` afterwards. On a
`worker_threads` Worker the identity answer is wrong. Three probes, same
branch, same box:

| what the Worker does | Node 26.5.1 | `main` @ `fcd108bfb` | integration @ `babc5f0d1f` | verdict |
|---|---|---|---|---|
| `fetch(url)` immediately | 200 | **200** | **`error: fetch failed`** | **regression** |
| `await setTimeout(50)`, then `fetch(url)` | 200 | **hangs** (25 s cap, rc=124) | **hangs** | pre-existing |
| `net.connect(...)` | OK | OK | OK | fine on both |

Read that table carefully, because it says two different things. The *failure*
is this branch's; the *hang* is not — a Worker that parks before fetching never
settles its promise on `main` either, which is a separate pre-existing defect
and a worse one (a server that fetches from a Worker stops rather than
erroring). And `net.connect` working on both is what says this is not "all
network I/O in a Worker": it is specific to the surfaces whose decline is
decided before the thread's loop state has settled.

So the "worker agents decline" story is right for `perry/thread` workers and
**wrong for `node:worker_threads`**, which is the one a Node program actually
uses. Both halves matter here and they pull in opposite directions:

* the tokio fallback is *less* reachable on this surface than the inventory
  would suggest, because nothing declines to it; and
* it is less reachable because the surface is broken instead, which is worse.

Attribution — whether this predates the turnloop lanes — is in "Perry defects
this work found" below.


## Linked, versus reachable at runtime

The brief asked for this distinction and it is worth the measurement, because
the two answers are different for almost every row above. Three facts, each
established by running something rather than by reading a manifest:

**1. A program that only uses the global `fetch()` reaches no tokio at all.**
`scripts/turnloop/apps/` style probe, one `await fetch(url)` against a local
origin, base compiler, `PERRY_LOOP_STATS=1`:

```
status 200 len 159323
[perry-loop] driver=turnloop turns=5 os_waits=3 native_ticks=0 completions=15
[perry-loop] p6 http_submitted=1 declined=0 completed=1 connects=1
[perry-loop-waits] arm=turnloop turnloop_waits=4 tokio_ticks=0 …
```

`declined=0` and `tokio_ticks=0` together are the claim: P6's engine served it
and the legacy tick never ran. Thread census: **1 thread**, before and after.

**2. `import axios` does *not* take the global fetch off turnloop — but axios
itself is on reqwest, in the same process.** Same probe plus
`await axios.get(url)`:

```
global status 200 / axios status 200
[perry-loop] p6 http_submitted=1 declined=0 completed=1
[perry-loop-waits] arm=turnloop turnloop_waits=4 tokio_ticks=1 tokio_tick_ns=13754149
```

One turnloop-served fetch and one tokio tick, in one program. This is the shape
the whole inventory has: linked *and* reachable, side by side, with the
transport chosen per call site rather than per program.

**3. The compiler's own tokio is linked into the compiler and never into a
compiled program.** `perry`'s reqwest/tokio/tokio-tungstenite serve `perry
publish`, `login`, `verify`, `audit`, `run --remote`, `setup ios|macos`, the
update check, telemetry and compat reports. A user binary links
`libperry_runtime.a`, `libperry_stdlib.a` and the `perry-ext-*` archives; it
does not link the `perry` crate. Those three edges still have to go for the
`Cargo.lock` goal, and no JS program is affected by when.

## Perry defects this work found (none of them P8's)

### 1. `import 'node-fetch'` makes the program's global `fetch()` SIGSEGV

Reduced to four lines, and deterministic — three runs, three
`Segmentation fault (core dumped)`:

```ts
// g1.ts
import nodeFetch from "node-fetch";
console.log("A: start, nodeFetch is", typeof nodeFetch);
const r = await fetch("http://127.0.0.1:8099/");   // <-- SIGSEGV here
console.log("B: typeof r =", typeof r);
```

```
A: start, nodeFetch is object
Segmentation fault (core dumped)            rc=139
segfault at 5 … Code: … 48 85 ff 74 2c … <8b> 57 04 …
```

`segfault at 5` with `mov 0x4(%rdi),%edx` and `rdi = 1` is a **handle id being
dereferenced as an object pointer**. Delete the one import and the identical
program is correct, on turnloop:

```
$ ./g0                      # same file, no node-fetch import
A: start / B: typeof r = object / C: status = 200 / D: len = 159336
[perry-loop] p6 http_submitted=1 declined=0 completed=1
```

**Cause, proven from the archives rather than inferred.** The well-known flip
maps `node-fetch` to `["http-client"]` and strips it — but
`compute_required_features` inserts **`web-fetch`** independently whenever
`uses_fetch` is true, and `web-fetch` is a different feature name from the one
that was stripped. So a program that imports `node-fetch` *and* calls the
global `fetch()` gets both archives, each defining the whole `js_fetch_*`
surface:

```
$ nm --defined-only target/perry-auto-f2a918410ade6174/release/libperry_stdlib.a
      js_fetch_get=1  js_fetch_response_status=1  js_headers_new=1
$ nm --defined-only target/perry-auto-f2a918410ade6174/release/libperry_ext_fetch.a
      js_fetch_get=1  js_fetch_response_status=1  js_headers_new=1
```

Two definitions, two response registries, and two **different handle
encodings** for the value the promise resolves with:

| | encoding |
|---|---|
| `perry-stdlib/src/fetch/mod.rs:406` | `handle_to_f64(id) = js_nanbox_pointer(id)` — NaN-boxed POINTER_TAG |
| `perry-ext-fetch/src/lib.rs:583` | `promise.resolve(JsValue::from_number(id as f64))` — a bare double |

The program that imports node-fetch but never calls the global `fetch()` gets
a *single* definition (`uses_fetch` is false, stdlib's are stripped) and does
not crash. It fails more quietly instead: `const r = await nodeFetch(url);
r.status` throws `TypeError: Cannot read properties of undefined (reading
'status')`. This lane did not chase that one to its cause — it may be the
bare-number handle above, or it may be that the awaited value's type is not
proven to be a `Response` at the property site — so it is reported as an
observation, not as a diagnosis.

The decisive control: the same source, the same compiler, one environment
variable apart.

| build | result |
|---|---|
| default (node-fetch → `perry-ext-fetch`) | **SIGSEGV** |
| `PERRY_DISABLE_WELL_KNOWN=1` (bundled copy) | `status = 200`, `p6 http_submitted=1 declined=0` |

**This is the same family as #10310** (`new Headers()` is a number because two
`js_headers_new` implementations disagree on the encoding) but it is a
different symbol, a different symptom and a wider blast radius: #10310 loses a
dynamic method call, this one kills the process on the single most common
network call in JavaScript. #10310's proposed fix — box the handle *and* add a
registered cross-boundary dispatch hook — does not by itself remove the
duplicate `js_fetch_*` definitions, so it would not fix this.

**Attribution: pre-existing on `main`, not a turnloop regression.** Measured,
not inferred — a `main` tree (`fcd108bfb`, v0.5.1579) was cloned and built from
source on the same box with the same package set, and the same `g1.ts`
compiled by it segfaults identically:

| | `g0` (no node-fetch import) | `g1` (node-fetch imported) |
|---|---|---|
| `main` @ `fcd108bfb` | `status = 200`, rc=0 | **rc=139**, 3/3 runs |
| `turnloop/integration` @ `babc5f0d1f` | `status = 200`, rc=0 | **rc=139**, 3/3 runs |

That agrees with the source: `perry-ext-fetch/src/lib.rs` and
`crates/perry/src/commands/stdlib_features.rs` are byte-identical to
`origin/main` on this branch, and `perry-stdlib`'s `handle_to_f64` was last
touched in August (#8448).

### 2. REGRESSION: `fetch()` inside a `node:worker_threads` Worker is broken on this branch

This is the finding the lane would report if it could report only one. It is a
regression of `turnloop/integration` against `main`, measured on both, three
runs each, and it is deterministic:

| | primary agent | inside a `Worker` |
|---|---|---|
| Node 26.5.1 | `status=200` | `status=200` |
| **`main` @ `fcd108bfb` (v0.5.1579)** | `status=200` | **`status=200`** |
| **`turnloop/integration` @ `babc5f0d1f`** | `status=200` | **`status=error:fetch failed`** |

Both Perry binaries were built from source in their own clone on the same box
with the same pinned toolchain, and the probe is the same source file compiled
by each; each probe's `perry-ext-*` archives were built by auto-optimize from
its own tree, so neither arm borrowed the other's. (The `main` tree's explicit
build omitted the five prebuilt `perry-ext-*` wrappers the gap suite needs —
it ran no sweep, only probes.) The commits `main` has that the integration
branch does not, v0.5.1577–1579, touch no file matching
`fetch|worker|agent|event_pump|turnloop`, so the difference is not a main-line
fix the branch is missing.

The counters say what happened:

```
[perry-loop] p6 http_submitted=2 declined=0 completed=1 failed=1 connects=1
```

Two requests submitted to P6's engine, **zero declined**, one completed (the
primary agent's), one failed (the Worker's). The Worker did not fall back to
reqwest — it was accepted by an engine whose loop it cannot own.

### 2b. The cause: `node:worker_threads` Workers never claim an agent id

`agent::enter_worker_agent()` is the only writer of `CURRENT_AGENT`, and its
three call sites are all in `crates/perry-runtime/src/thread.rs` —
`perry/thread`'s `spawn`, `parallelMap` and `parallelFilter`.
`crates/perry-stdlib/src/worker_threads.rs:1273` spawns the Worker's OS thread
with a bare `std::thread::spawn` and never calls it, so a `worker_threads`
Worker resolves to `PRIMARY_AGENT` for the whole of its life.

Everything that asks "am I the primary agent?" therefore gets the wrong
answer on a Worker. `turnloop_net::available()` is one of those things:

```rust
// crates/perry-stdlib/src/turnloop_client/mod.rs:428
pub(crate) fn submit(spec: RequestSpec, sink: Sink) -> Result<(), Declined> {
    if !tl::available() {              // true on a Worker — see above
        return Err(Declined::NoLoop);  // the fallback that never happens
    }
    …
    SUBMITTED.fetch_add(1, Ordering::Relaxed);
    exchange::start(id);               // submits to a loop this thread has none of
```

`ensure_loop_with` *does* refuse a moment later — `PRIMARY_ROUTE` is already
held by the main thread, so the Worker is marked `LoopState::Declined` — but
that is after the request was accepted, so the caller gets a failure instead of
the reqwest fallback the design intends. On `main` there was no engine to
accept it and the Worker's fetch ran on the shared runtime, which is why it
worked.

This is why the decline is worth measuring rather than asserting: eight lane
reports describe a fallback that, on the agent kind a Node program actually
uses, is not reached.

Two other things read the same predicate and are worth checking under it,
though this lane did not: the keep-alive accounting (`owns(owner)` decides
which thread may drain a queue entry) and `class_image.rs`'s comment, which
already notes that `CURRENT_AGENT` defaults to the primary and keys around it.

The fix is not obviously one line, which is why this lane reports rather than
applies it. Calling `enter_worker_agent()` in the Worker's thread body makes
every turnloop surface decline on a Worker — which is the documented intent and
would repair `fetch` — but it also switches net, TLS, the HTTP server, SMTP and
all four database drivers onto their tokio paths there for the first time, on a
surface with almost no gap-suite coverage. The alternative reading is that the
predicate is wrong rather than the agent id: "can this thread own a loop" is a
question about `PRIMARY_ROUTE`, not about agent identity, and `submit` checks
`tl::available()` *before* `ensure_loop_with` has had a chance to say no.
Whichever is chosen, it wants its own change with its own oracle run.

### 3. A `Worker` whose entry is its own module does not link

```
undefined reference to `tokio_worker_agent_census_ts__init_body'
        referenced from `perry_closure_tokio_worker_agent_census_ts__11'
```

`new Worker(new URL(import.meta.url))` — the idiomatic Node form for a
self-hosting worker, and what this lane's census probe was first written as —
fails at link time. Node 26.5.1 runs the same file correctly. The same program
with the worker body in a separate file
(`test-files/test_gap_9744_static_worker_helpers.ts`'s shape) links and runs,
which is what the committed probe now does.

Not investigated further: it is a codegen/emission bug, not a transport one,
and it is outside this lane. It is named here because the workaround is in a
committed file and a reader of that file is entitled to know why it is two
files instead of one.


### 4. `scripts/gc_runtime_root_holders.py` is red on the integration branch

`lint` runs it, `lint` is part of the required `pr-gate`, and it fails on
`turnloop/integration` before this branch changes anything:

```
gc_runtime_root_holders: these inventory entries no longer match an
uncovered holder. Delete them — a stale exemption is how this gate stops
being one.

  crates/perry-ext-http/src/server/turnloop_serve/conn.rs | CONNS | turnloop P5. …
```

Verified as pre-existing rather than assumed: restoring the base commit's
`cron.rs` (this branch's only Rust change, and in a different crate) leaves the
same single failure. The entry was added by P5 and has since become *covered*,
which the gate's own text says "is exactly what a fix looks like" — so the fix
is to delete that entry, and it belongs to whoever owns P5's change rather than
to this lane.

## The compile-time A/B switch (`tokio-wait-driver`)

The brief asked what removing it would take, and said the decision is the
coordinator's. Inventory, exactly:

| | |
|---|---|
| declared | `perry-stdlib/Cargo.toml` → `perry-runtime/tokio-wait-driver` (both are no-op features; nothing else may depend on them) |
| `#[cfg]` guards | **27** — 25 in `crates/perry-runtime/src/event_pump.rs` (its test module included), 2 in `crates/perry-stdlib/src/common/async_bridge.rs` |
| runtime `cfg!()` reads | **3** — two in `event_pump.rs`'s stats banner, one in `perry-stdlib/src/readline/pump.rs` |
| named in comments only | 19 further files (`perry-ffi`, `perry-ext-{net,pg,mysql2,ioredis,nodemailer}`, `perry-stdlib/src/{fetch,nodemailer,turnloop_client,turnloop_smtp}`, `perry-runtime/src/{turnloop_net,turnloop_pool,turnloop_proc,dgram_reactor,child_process}`) — each says "this declines in the `tokio-wait-driver` arm", none compiles differently |

What it does: with the feature on, `event_pump::net_loop_available()` returns
`false` unconditionally, `register_wait_driver` returns `None`, and every agent
parks through the legacy tokio tick. **Every turnloop surface then declines**,
which is why 19 further files mention it in comments — it is the second arm of
exactly the same predicate the worker-agent decline is the first arm of.

Removing it is a small, mechanical change — delete the two feature
declarations, take the `#[cfg(not(...))]` branch at all 27 sites, fold the
three `cfg!()` reads, delete the `#[cfg(feature = ...)]` bodies, and reword 19
files' comments. It is *not* small in consequence, and the argument cuts both
ways:

* **For removing it now.** CLAUDE.md's kill-policy: "a mode that still exists
  is a decision that hasn't been made", and no CI arm exercises the feature-on
  state. P6 recorded that it did not even *build* the arm, only reasoned that
  it should work. That is precisely the shape the policy exists to forbid.
* **Against removing it now.** It is the measurement instrument for the
  published P0–P7 A/B, which the coordinator says is still running on the quiet
  mini. Deleting the "before" arm before the comparison is published destroys
  the only reproducible baseline.

The honest reading is that both are true and they are sequenced: the switch
should go the day the A/B numbers are published, not before, and it should go
in **one** commit that also deletes the dead branch bodies rather than leaving
`cfg(not(...))` scaffolding behind. P4's report also flags a trap for whoever
uses it in the meantime: for P4's own subjects the feature arm is **not** a
faithful "before", because with the loop off those subjects take the *inline*
fallback rather than the tokio pool they used pre-P4.

## What it would cost to actually remove tokio

Ordered by dependency, not by size: **A is the precondition for B, C, I and
every "on a declining path" row in the inventory**, and L cannot happen until
everything above it has.

Every one of the 46 edges appears in exactly one row, and the `edges` column
sums to 46. That is not an assertion: each edge carries its group in
`scripts/tokio_inventory.json`, and `--list` prints the totals, so the
arithmetic is re-derived from the tree every run:

```
$ python3 scripts/tokio_inventory.py --list | tail -16
removal-plan groups (docs/turnloop/p8-report.md), 46 edges in 14 groups:
  A    4      F    4      K    2
  B    8      G    5      L    1
  C    2      H    6      M    1
  D    2      I    3      N    1
  E    4      J    3
```

A plan whose parts do not add up to the whole is a plan that discovers a
fifteenth item late.

| # | work | edges it removes | cost |
|---|---|---|---|
| **A** | **Per-agent `turnloop::Loop`s.** `ensure_loop_with` declines every non-`PRIMARY_AGENT` thread; give each agent its own loop, poster and timer heap, and a per-agent notify route (`PRIMARY_ROUTE` stays for `js_notify_main_thread`). Fix the `worker_threads` agent-id defect with it, or the two interact. | **4** — `perry-ext-net` × 2 outright; `perry-ext-http`'s `hyper` + `hyper-util` need **A and E and the cluster fix** together, because the declining server has three causes (no loop, an attached `WebSocketServer`, a cluster worker's `SO_REUSEPORT` bind) | a phase. Plus validation nobody has done: every P1/P5/P6/P7 surface starts taking the turnloop path on a worker for the first time. The cluster case also needs PerryTS/turnloop#49 |
| **B** | **TLS, UDS and topology from a database binding** — `perry_db_turnloop` → `turnloop-tls`, a `pipe_connect` for pg's Unix socket, SRV + SDAM for mongo. Gated on A. | **8** — `perry-ext-{ioredis,pg,mysql2,mongodb}` × 2 | a phase. #10335 makes it urgent: the default `new Redis()` configuration already declines |
| **C** | **The `node:http`/`node:https` client** — `agent.rs`'s ~1,950-line Node-semantics pool over reqwest's, plus three raw-`tokio::net::TcpStream` bypasses (`TE: trailers`, `Expect: 100-continue`, `agent.createConnection`) | **2** — `perry-ext-http`'s `reqwest` and `tokio-rustls` | a phase on its own; P6 said so and P8 agrees. #10328 rides along |
| **D** | **HTTP/2**, both halves, onto `turnloop_http::http2::Connection` | **2** — `h2`, and `perry-ext-http`'s `tokio` (its last, once C and E are done) | a phase (#10327) |
| **E** | **`ws`** onto `turnloop-websocket` — a tungstenite **major-version** migration of the stored connection type, not a transport swap | **4** — `perry-ext-ws` × 2, `perry-ext-http`'s and `perry-stdlib`'s `tokio-tungstenite` | medium; unblocks P5's attached-`WebSocketServer` hole at the same time |
| **F** | **`fastify`** — needs either a dependency edge to perry-ext-http or a new crate holding the sans-I/O server | **4** — `perry-ext-fastify` × 4 | medium |
| **G** | **`axios` and `node-fetch`** — a `js_perry_http_*` C seam of the shape P6 built for SMTP. The duplicate-`js_fetch_*` defect above has to be fixed first, or the seam is built on a SIGSEGV | **5** — `perry-ext-{axios,fetch}` × 4, `perry-stdlib`'s `reqwest` | medium; the seam is the bounded part, the two crates' own defects (#10310, #10325, #10326) are not |
| **H** | **`perry-stdlib`'s bundled `pg`/`mysql2`/`ioredis`/`mongodb`, its `ws` module and its hyper framework server** — all compiled out of every default build, so this is a policy call about whether the fallback stays, not a transport one | **6** — `perry-stdlib`'s `sqlx`, `redis`, `mongodb`, `hyper`, `hyper-util`, `tokio-rustls` | small as code, a decision as policy. It is the cheapest lockfile reduction in the tree |
| **I** | **lettre's async transport** — lets `bundled-nodemailer` drop `tokio1` / `tokio1-rustls-tls` / `pool` and keep only the MIME builder, which stays forever (`turnloop-smtp` re-exports it). Gated on A. | **3** — `perry-ext-nodemailer` × 2, `perry-stdlib`'s `lettre` | small |
| **J** | **The `perry` CLI** — `publish`, `login`, `verify`, `audit`, `run --remote`, `setup`, the update check, telemetry, compat reports. 14 `reqwest::Client` constructions (7 blocking, 7 async) across 11 files, 7 `Runtime::new` sites, 2 WebSocket clients | **3** — `perry` × 3 | medium, and it needs multipart in `turnloop-http`'s client, which does not have it |
| **K** | **`perry-container-compose`** — the engine behind `perry/container`, `perry/compose` and `perry/workloads` (perry-stdlib's `container` feature) and the `perry-compose` binary | **2** — `perry-container-compose` normal + dev | **done.** Not a rewrite: the async code stays async; its leaves (the backend CLI child processes, timeouts, the async mutex) moved onto a turnloop-backed `rt::block_on`, and `container` needs only the promise bridge |
| **L** | **`perry-stdlib`'s `tokio`** — the `async-runtime` feature, `common::async_bridge`, and the `perry_ffi_spawn_blocking*` / `spawn_async` C ABI | **1** — the last edge | falls out of A–K; see below |
| **M** | ~~**`perry-ui-gtk4`** — `ksni` and `mpris-server` *require* tokio~~ — **this was wrong, and the edge is gone.** Neither crate requires tokio. `ksni`'s `async-io` feature is a first-class alternative to its `tokio` default (the two are mutually exclusive — `ksni::compat` has a `compile_error!` if both are on) and carries its own executor thread; `mpris-server`'s `tokio` feature is opt-in, is not in its defaults, and only forwards to `zbus/tokio`, which zbus needs no more than any of its other executor backends. Perry had asked for both features and then kept a direct tokio dependency to feed them. Removed with tray and MPRIS intact — `docs/turnloop/gtk4-report.md` | **1** | **done.** No crate replaced, no capability dropped |
| **N** | **`perry-ui-android`'s `tungstenite`** — synchronous tungstenite (0.24 when this was written, 0.30 since #11065) on a std background thread per connection, with no tokio anywhere in its graph. **Not a tokio edge**; listed because it pins the third tungstenite major in the tree, which is part of E's cost | **1** | small, and only worth doing with E |
| | | **46** | |

### Why L is genuinely last, and not a layer you can lift out first

`perry-stdlib`'s `async-runtime` feature gates `common::async_bridge` and
`perry_ffi_async` — the whole promise bridge, and the C ABI every
`perry-ext-*` crate settles its promises through. Twenty other features imply
it. It reads like the bottom layer, and it is not a layer at all:
`async_bridge::RUNTIME` is a tokio current-thread runtime **because its clients
hand it tokio futures**. `spawn`, `block_on`, `run_one_tick` and
`drive_pending` exist to drive reqwest, hyper, tokio-tungstenite, sqlx, the
`redis` crate, the `mongodb` driver and lettre. Remove the last of those and
the bridge's tokio has no work; remove the bridge first and there is nothing to
run them on.

P4's three v1 shims are the same shape and are the concrete blocker:
`perry_ffi_spawn_blocking`, `_with_reactor` and `spawn_async` cannot move onto
turnloop's pool because their remaining callers — the HTTP/2 accept loop, the
HTTP/2 client, and every database binding running `Handle::current().block_on`
— hold a thread for a **connection's** lifetime, not a job's, and turnloop's
pool is bounded and fixed-size by design. That is upstream
**PerryTS/turnloop#42**, and until it lands (a detached/long-occupancy job
class, or a second pool) the shims stay whatever else moves.

## What this lane changed

Three things, and the first is the one that matters.

### 1. `scripts/tokio_inventory.py` + `scripts/tokio_inventory.json`, in `lint`

Described above. It is wired into the **existing** required `lint` job rather
than added as a new required context, which sidesteps the trap CLAUDE.md names:
a brand-new gate has never been green, so promoting it to required immediately
blocks every open PR. A new *step* in a job that is already required gates from
the first run with no branch-protection change.

`--self-test` plants seven changes and requires the checker to catch each one:
a new edge, a stale entry, a package entering the lockfile, a package leaving
it, a version change, a target-gated edge (the shape `cargo tree` hides), and
an optionality flip (`optional = true` → `false` changes which builds link it,
so it is a different edge, not the same one).

```
$ python3 scripts/tokio_inventory.py --self-test
tokio_inventory self-test: OK (7 planted changes, all caught)
```

The self-test drives synthetic metadata, so it was also proved end to end
against the real tree: adding `tokio = { workspace = true }` to
`crates/perry-ext-qs/Cargo.toml` — a crate that has no tokio today —

```
tokio inventory gate FAILED:
  - NEW tokio edge: perry-ext-qs -> tokio (kind=normal, optional=False, target=None).
    Perry is migrating OFF tokio; adding an edge needs an entry in
    scripts/tokio_inventory.json saying which JS surface reaches it and what
    blocks its removal.
exit=1
```

and reverting it returns the gate to green. A gate that has only ever been
green is a gate nobody has seen fail.

### 2. `perry-stdlib`'s cron helpers no longer spawn a task that cannot fire

`js_cron_set_interval` and `js_cron_set_timeout` each spawned a native task
whose entire body was

```rust
// Invoke callback (in real impl: js_callback_invoke(callback_id))
```

— the placeholder `cron.rs`'s own module header records as the bug
`js_cron_schedule` was rewritten to fix. The task could not do the thing it
existed for, and the interval's loop only observes its cancel flag *after* the
next sleep, so a cleared 24-hour interval held a tokio task for a day.

Reaching either would have been a defect, and reaching them is also impossible:
codegen declares the four symbols in `runtime_decls/stdlib_ffi/third_party.rs`
and **no lowering path emits a call to any of them** (`setInterval`/`setTimeout`
lower to the runtime timer heap; the npm `cron` surface lowers to
`js_cron_schedule` / `js_cron_job_*`). `perry-ext-cron` — the copy the
well-known flip actually links for `import 'cron'` — has always been a bare
handle allocator.

So the spawn is removed rather than migrated, and the two copies of the same
four symbols now agree. This is CLAUDE.md's kill-policy call, not a transport
one. `cron.rs` now has no tokio call sites. It removes no manifest edge and the
report does not claim otherwise.

### 3. `scripts/turnloop/apps/tokio_worker_agent_census.ts`

The probe for the decline in "The one blocker" above: the same `fetch`, once on
the primary agent and once inside a `worker_threads` Worker, with the OS thread
names read out of `/proc` both times. tokio names its pool `tokio-rt-worker`;
turnloop names its blocking threads `turnloop-blocki`. Linux only, and it
prints `unavailable` rather than a zero that would read as "no tokio".

## turnloop gaps found

Second-hand rather than first-hand — this lane wrote no turnloop code — but
each is a concrete thing Perry needs and does not have. The coordinator files
them.

1. **A long-occupancy job class, or a second pool** (already **PerryTS/turnloop#42**,
   raised by P4). Restated here only because it is load-bearing for this lane's
   conclusion: it is the reason `perry_ffi_spawn_blocking*` and `spawn_async`
   cannot move, and therefore the reason `perry-stdlib`'s `tokio` edge is last
   rather than first.
2. **`turnloop_http::client` has no multipart form builder.** `perry publish`,
   `perry audit`, `perry verify` and `perry run --remote` all post
   `reqwest::multipart::Form`s. Without multipart, moving the CLI off reqwest
   means hand-rolling RFC 7578 in Perry — which is the kind of thing that
   belongs in the HTTP crate, next to the chunked encoder it already has.
3. **`turnloop-websocket` is on tungstenite 0.30; Perry stores tungstenite 0.29
   types** (`perry-ext-ws`, `perry-ext-http`, `perry-stdlib`), and
   `perry-ui-android` carries a third major (0.24, sync). This is the whole
   reason `ws` and the attached-`WebSocketServer` path did not move in P5, and
   it is not a transport problem — it is a stored-type problem. Either a
   compatibility shim or an explicit statement of the intended migration order
   would unblock three surfaces at once.

## Test evidence

All commands as run.

### Local gates

Run from the branch, on the macOS development host:

| gate | result |
|---|---|
| `cargo fmt --all -- --check` | OK |
| `./scripts/check_file_size.sh` | OK |
| `python3 scripts/addr_class_inventory.py` | OK |
| `python3 scripts/check_test_registration.py` | OK |
| `python3 scripts/check_node_version_consistency.py` | OK |
| `python3 scripts/tokio_inventory.py --self-test` | OK — 7 planted changes, all caught |
| `python3 scripts/tokio_inventory.py` | OK |
| `python3 scripts/gc_runtime_root_holders.py` | **FAIL — pre-existing, see defect 4** |
| `bash scripts/run_lint_gates.sh --list` | picks up the new step: 81 lint commands from 46 run steps |
| `bash scripts/run_lint_gates.sh --self-test` | OK |

`cargo check -p perry-stdlib --no-default-features --features full` is clean,
and the release build of the full package set below produced three warnings,
all of them the pre-existing `redis v1.6.0` future-incompatibility note.

### The gap suite, against a baseline built from this branch's own base

Both arms were built from source in their own tree on the build box, from the
same package set, and both sweeps ran against the pinned oracle Node 26.5.1:

```
cargo build --release --locked \
  -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static \
  -p perry-ext-http -p perry-ext-net -p perry-ext-ws -p perry-ext-zlib -p perry-ext-events
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh
```

The baseline is `babc5f0d1f` — this branch's own base — in its own clone
(`/root/claude-turnloop-p8/base`), because the committed snapshot cannot be
assumed to agree with it. The P8 arm's binaries were built at `4a3966747`;
every commit after it on this branch touches only `docs/`, `scripts/`,
`changelog.d/` and `.github/`, verified with
`git diff --stat 4a3966747..HEAD -- crates/ Cargo.toml Cargo.lock` (empty), so
the swept binary is HEAD's code. Three tests
(`2899_2779_2777_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch`) are red against the committed snapshot on the
base commit before this branch changes anything, which is exactly why the
comparison is arm-against-arm.

| | base `babc5f0d1f` | **P8** (`4a3966747`) |
|---|---|---|
| tests run | 807 | 807 |
| pass | 797 | **798** |
| parity_fail | **9** | **9 — the same nine** |
| compile_fail | 0 | **0** |
| crash | 0 | **0** |
| node_fail | 1 | 0 |
| parity rate | 98.8 % | 98.8 % |
| **status changes, compared per test** | — | **1, and it is the oracle's** |

Compared from the two JSON reports test by test, not from the totals. The two
runs share all 807 test ids, and exactly one differs:

```
STATUS CHANGES on the common set: 1
   test_gap_9536_fetch_url_error: node_fail -> pass
```

`node_fail` means **Node** exited non-zero, not Perry — that fixture drives
`fetch` at unreachable hosts and reads the `cause` diagnostics back, so it
depends on the machine's resolver, and the base sweep caught it while the box
was running two sweeps and three cargo builds. Re-run afterwards on the same
box, the oracle passes 3/3 (`node --experimental-strip-types
test_gap_9536_fetch_url_error.ts` → rc=0 each time). A compiler change cannot
alter whether Node exits non-zero, so the change is environmental and it is in
the arm's favour, which is the direction that cannot hide a regression.

**Zero Perry-side status changes in either direction.** The nine parity
failures are byte-identical sets:

```
2159_defineproperty_class_prototype   json_lazy_defineproperty_index
2514_settracesigint                   perfhooks_3088_3008_3010_3011
2899_2779_2777_static_helpers         prop_plan_cache_invalidation
disposablestack_2875                  v8_2_3680plus
iterator_prototype_next_patch
```

— the same nine P6 and P7 recorded, none of them this branch's. Both arms exit
non-zero for the same reason: three of those nine (`…_static_helpers`,
`disposablestack_2875`, `iterator_prototype_next_patch`) are expected to PASS
by the committed snapshot and are red on the base commit before this branch
changes anything.

### Probes

Each probe names the compiler that built it, and each was compiled from that
tree's own `target/release` with `PERRY_RUNTIME_DIR` pointed at it. "base" is
`turnloop/integration` @ `babc5f0d1f` in `/root/claude-turnloop-p8/base`; "P8"
is this branch in `/root/claude-turnloop-p8/perry`; "main" is `fcd108bfb`
(v0.5.1579) in `/root/claude-turnloop-p8/mainref`. The base and P8 arms differ
by exactly the `cron.rs` change, which no probe here touches — so a probe built
by one is evidence about the other for every subject in this report.

| probe | built by | what it establishes |
|---|---|---|
| `fetch_only.ts`, `g0.ts` | base | the global `fetch` is on turnloop: `p6 http_submitted=1 declined=0`, `tokio_ticks=0`, one thread |
| `fetch_with_axios.ts` | base | axios does **not** take the global fetch with it — one turnloop fetch and one tokio tick in one process |
| `g1.ts` (= `g0.ts` + `import 'node-fetch'`) | base **and** main | SIGSEGV, 3/3 on each — the crash is pre-existing, not a turnloop regression |
| `g1_nowk` (same file, `PERRY_DISABLE_WELL_KNOWN=1`) | base | correct (`status=200`) — isolates the crash to the well-known routing |
| `nf_only.ts` | base | node-fetch alone: `r.status` is `undefined`, a bare-number handle |
| `scripts/turnloop/apps/tokio_worker_agent_census.ts` | P8 **and** main | the Worker-agent regression: 200 on main, `fetch failed` on the branch, 3/3 each |
| `netw_main.ts` | P8 **and** main | `net.connect` inside a Worker: OK on both — the regression is fetch-specific |
| `race.ts` (park, then fetch, inside a Worker) | P8 **and** main | the fetch **hangs** on both — never settles, never rejects. Pre-existing, not this branch's |

### The sweep does not exercise this branch's only runtime change — and that is the point

The evidence standard asks for proof that the subject ran. Here it is the other
way round, and saying so is more useful than a counter that would be zero
either way: this branch's only runtime change removes two `spawn_native` calls
from four symbols **nothing lowers to**. No gap fixture can reach them, and no
`PERRY_LOOP_STATS` counter can show them running, because the whole argument
for removing the spawn is that it was unreachable. What the sweep establishes
is the complementary thing — that removing them changed nothing that *is*
reachable.

The two pieces of evidence that the removal is safe are static, and both are in
"What this lane changed": no lowering path emits `js_cron_set_interval` /
`js_cron_set_timeout` / their clear-counterparts, and `perry-ext-cron` — the
copy `import 'cron'` actually links — already behaves exactly as the stdlib
copy now does.

### What was not run

See "What P8 did not do". In particular: no Windows or macOS end-to-end run, no
benchmark, no `cargo test --workspace`, and no GC-stress arm — this branch adds
no code that holds a JS value across a thread or a completion, so there is no
subject for `PERRY_GC_SCHEDULE_SEED` to stress. Saying that plainly is better
than running the knob over a change it cannot reach and reporting a green.

## What P8 did not do

Named precisely, because each is a hole rather than a preference.

* **It moved nothing off tokio.** Not one of the 46 edges is gone, and the
  report says so in its first paragraph rather than in a footnote. The three
  changes it does make are an instrument, a dead-code subtraction and a probe.
* **It did not attempt per-agent loops**, which is the item its own analysis
  says is highest-leverage. That is a deliberate call: switching every
  P1/P5/P6/P7 surface onto turnloop on worker threads, on a surface with almost
  no gap-suite coverage, without the per-subsystem validation each of those
  lanes did for the primary agent, is exactly the kind of change that is
  discovered in production. It is a phase.
* **It did not investigate the second Worker defect it uncovered** — a Worker
  that parks before fetching never settles the promise, on `main` as well as on
  this branch. It is named and measured, and it is not this branch's, so it
  wants its own issue and its own lane.
* **It did not fix the `worker_threads` agent-id defect** it found, for the
  same reason and with the added complication that there are two defensible
  fixes (see the defects section) and choosing between them needs an oracle run
  this lane did not have time for. It also did not check the other readers of
  the same predicate — the keep-alive accounting's `agent::owns`, and
  `class_image.rs`, whose own comment already notes it keys around
  `CURRENT_AGENT` defaulting to the primary.
* **It did not build the `js_perry_http_*` C seam** that would move `axios` and
  `node-fetch`. P6 costed it as "a second ABI's worth of design"; P8 agrees, and
  adds that the duplicate-`js_fetch_*` defect has to be fixed first or the seam
  would be built on top of a SIGSEGV.
* **It did not delete the `tokio-wait-driver` switch.** The brief reserves that
  for the coordinator, and the A/B it exists for is still running.
* **It did not delete `perry-stdlib`'s bundled database modules**, which is the
  single cheapest lockfile reduction available (it removes `sqlx`, `redis` and
  `mongodb` from that crate). They are the fallback when a wrapper archive
  cannot be built, so removing them is a policy decision about release archives,
  not a transport one.
* **Windows and macOS.** Everything here ran on Linux x86_64. The inventory
  script itself runs on both (it is manifest-reading Python, and it is written
  to report target-gated edges precisely because the development host is macOS),
  but no probe and no sweep ran off Linux.
* **Any benchmark.** The build box was running two gap sweeps and three cargo
  builds throughout, and the brief forbids timing there. No number in this
  report is a performance claim; the `[perry-loop]` counters are liveness
  evidence, not measurements.
* **The auto-optimize gap tier** as a whole. The fast tier ran, which itself
  takes the auto-optimize path per ext-routed test (#7629) — 35 of the
  `test-files/test_gap_*.ts` fixtures import a module the well-known flip
  routes to a `perry-ext-*` wrapper, and each of those built its own coherent
  archives — but the 8-shard auto-optimize mode did not run.
* **`cargo test --workspace`**, and the `perry-runtime` unit suite. This
  branch's only Rust change is the removal of two `spawn_native` calls from
  `cron.rs`; `cargo check -p perry-stdlib --no-default-features --features full`
  is clean and the release build of the full package set is clean, but the unit
  suites were not run.

## For the integrator

The branch is `turnloop/p8-detokio` on `origin`. Nothing here bumps the
version. The changelog fragment is
`changelog.d/turnloop-p8-tokio-inventory.md`.

Run, on a machine with the pinned oracle installed:

```bash
# the gate, and proof it can fail
python3 scripts/tokio_inventory.py --self-test
python3 scripts/tokio_inventory.py
python3 scripts/tokio_inventory.py --list      # the census
python3 scripts/tokio_inventory.py --table     # the annotated inventory

# the only Rust change
cargo check -p perry-stdlib --no-default-features --features full

# the gap suite, against a baseline from this branch's OWN base commit
cargo build --release --locked \
  -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static \
  -p perry-ext-http -p perry-ext-net -p perry-ext-ws -p perry-ext-zlib -p perry-ext-events
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh

# the worker-agent census (needs an HTTP origin; Linux only)
python3 -m http.server 8099 --bind 127.0.0.1 &
perry scripts/turnloop/apps/tokio_worker_agent_census.ts -o wcensus
PERRY_LOOP_STATS=1 ./wcensus
node --experimental-strip-types scripts/turnloop/apps/tokio_worker_agent_census.ts
```

Three decisions are yours, not this lane's:

1. **The `tokio-wait-driver` switch.** Delete it the day the P0–P7 A/B is
   published, in one commit that also removes the dead branch bodies.
2. **`perry-stdlib`'s bundled `pg`/`mysql2`/`ioredis`/`mongodb` copies.**
   Deleting them removes `sqlx`, `redis` and `mongodb` from that crate — the
   cheapest lockfile reduction available anywhere in the tree — at the cost of
   the fallback used when a wrapper archive cannot be built.
3. **Whether the tokio inventory gate stays in `lint`.** It is there now, it is
   green, and its `--self-test` proves it can fail. If it is removed, the
   migration goes back to being measured by prose.

And five things want issues filed, none of which is a duplicate of the 47
already open:

1. **`fetch()` inside a `node:worker_threads` Worker fails on this branch and
   works on `main`** — a regression, and the one thing that should block the
   merge.
2. **`node:worker_threads` Workers never claim an agent id**, which is its
   cause and which every other reader of `current_agent()` inherits.
3. **A Worker that parks before fetching hangs forever** — measured on `main`
   too, so a separate pre-existing defect, and a worse one than (1).
4. **`import 'node-fetch'` + the global `fetch()` SIGSEGVs**, from two linked
   definitions of every `js_fetch_*` symbol. Pre-existing on `main`; related to
   #10310 but not the same bug and not fixed by #10310's proposed fix.
5. **Per-agent `turnloop::Loop`s**, as the tracking issue for the item that
   gates most of the remaining migration.

Two more, smaller: `new Worker(new URL(import.meta.url))` does not link, and
`scripts/gc_runtime_root_holders.py` is red on `turnloop/integration` with a
stale P5 entry.
