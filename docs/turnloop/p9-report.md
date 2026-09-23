# turnloop P9 — a loop per JS agent

Perry created exactly one `turnloop::Loop` and it belonged to the primary
agent. Every migrated subsystem sat behind one predicate:

```rust
// crates/perry-runtime/src/event_pump/agent_loop.rs, before this lane
pub(super) fn net_available() -> bool {
    match STATE.with(Cell::get) {
        LoopState::Owner => true,
        LoopState::Declined | LoopState::ShutDown => false,
        LoopState::Unset => crate::agent::current_agent() == crate::agent::PRIMARY_AGENT,
    }
}
```

`turnloop_net::available()` is that function, and `perry-ext-net`,
`perry-ext-http`'s server, perry-stdlib's fetch bridge, its SMTP bridge and all
four database drivers gate on it — directly or through `perry_db_turnloop`'s
`enabled()`. On any JS thread that is not the primary agent it answered `false`,
so a `node:worker_threads` Worker's `fetch` went to reqwest, its `net.connect`
to a tokio `TcpStream`, and its `new Redis()` to the `redis` crate. That
fallback is live code, not dead code, which is why tokio could not be deleted:
P8's removal plan puts it first, as group **A**, and every group from **B** on
is gated on it.

It no longer mentions `PRIMARY_AGENT`. The question is now "do I have (or may I
take) a loop", which is true on every thread that runs a JS agent's event loop.

## The headline

The acceptance case, on the box, one process, `PERRY_LOOP_STATS=1`:

```
$ PERRY_LOOP_STATS=1 ./p9_worker_agent_acceptance          # THIS BRANCH
primary-agent fetch: status=200 bytes=160
primary-agent connect: echo="p9"
primary-agent database: set=OK get=p9-value del=1
worker-agent fetch: status=200 bytes=160
worker-agent connect: echo="p9"
worker-agent database: set=OK get=p9-value del=1
done
[perry-loop] driver=turnloop turns=21 os_waits=10 ... native_ticks=0 turn_errors=0 completions=32 ... agent=1
[perry-loop] driver=turnloop turns=42 os_waits=20 ... native_ticks=0 turn_errors=0 completions=32 ... agent=0
[perry-loop] p6 http_submitted=2 declined=0 completed=2 failed=0 connects=2 reused=0 ...
[perry-loop-waits] arm=turnloop turnloop_waits=36 ... tokio_ticks=0 tokio_tick_ns=0 ...

$ PERRY_LOOP_STATS=1 ./p9_worker_agent_acceptance          # BASE, 1edb5b7e8d
primary-agent fetch: status=200 bytes=160
primary-agent connect: echo="p9"
primary-agent database: set=OK get=p9-value del=1
worker-agent fetch: status=200 bytes=160
worker-agent connect: echo=""
worker-agent database: set=undefined get=undefined del=undefined
done
[perry-loop] driver=turnloop turns=24 os_waits=11 ... native_ticks=9 turn_errors=0 completions=32 ...
[perry-loop] p6 http_submitted=1 declined=1 completed=1 failed=0 connects=1 reused=0 ...
[perry-loop-waits] arm=turnloop turnloop_waits=18 ... tokio_ticks=9 tokio_tick_ns=2636677 ...
```

Read the two stderr blocks rather than the two stdout blocks, because the stdout
difference understates it. On the base commit there is **one** `[perry-loop]`
line and it has no `agent=` field: one loop, the primary's. `p6
http_submitted=1 declined=1` says the Worker's `fetch` was **refused** by the
turnloop client and went to reqwest, and `tokio_ticks=9` says the primary agent
had to take nine tokio ticks to drive that Worker's tokio-side work. On this
branch there are **two** lines, `agent=1` and `agent=0`; the Worker's own loop
ran 21 turns and dispatched 32 completions with `native_ticks=0`,
`http_submitted=2 declined=0`, and the process took **no tokio tick at all**.

The stdout difference is worth naming too, because it is larger than "the same
work on a different transport". On the base commit the Worker's `net.connect`
returns `echo=""` -- the socket never delivered a byte -- and its three Redis
calls all return `undefined`. Those are not slower answers, they are wrong ones.
A Worker's raw socket and its database round-trip did not work at all before
this lane; they were not merely running on tokio.

## What changed

### 1. One route per agent, claimed before the loop exists

`PRIMARY_ROUTE` was a single `{ in_turn: AtomicBool, notifier: Mutex<Option<(u64, Notifier)>> }`.
It is now `ROUTES`, one `Route` per agent, holding that agent's id, the
`ThreadId` that owns it, the loop identity behind it, an `Arc<AtomicBool>`
saying whether the owner is inside `turn`, and the `Notifier` once a loop
exists. `PARKED_LOOPS` is a count of agents currently inside a turn, so a wake
producer's fast path is the same single atomic load it was before.

The slot is claimed **before** the loop is built, and that ordering is the
point rather than an implementation detail. `net_available()` and
`ensure_loop_with()` are two predicates over the same question, and they
disagreed once: `c13372cc70` exists because a `worker_threads` Worker reported
`PRIMARY_AGENT`, the submit guard accepted its `fetch()`, and `ensure_loop_with`
refused a moment later — so the request **failed after acceptance** instead of
taking the fallback the design intends. If `net_available()` answered from
agent identity again, or optimistically, that class would come straight back
the first time two threads raced for one agent's route. A claimed slot cannot
be taken away, so a `true` from `net_available()` is a promise the loop
creation can keep.

The cost is one mutex acquisition per thread, on the first ask, and a TLS read
for every ask after it — `LoopState` gains a `Claimed` value between `Unset` and
`Owner`.

### 2. Exactly one thread per agent, first to ask

A second thread acting for an agent another thread already owns is declined and
keeps the legacy park. This is not a new restriction; it is the rule
`ensure_loop_with` already enforced for the primary agent ("a second thread
acting for the primary agent (a host pump thread) … keeps the legacy park"),
generalised. It exists for Android, where the compiled TypeScript runs on the
`perry-native` thread and the timer/microtask pump fires from the UI thread via
`nativePumpTick`: two threads, one agent. `perry-native` runs the event loop, so
it asks first and owns the loop; the UI thread keeps the behaviour it has today.

The tie-break is deliberately the same one the code had before — first to ask —
rather than something cleverer, because the thread that asks first is the thread
running that agent's event loop, and a loop owned by a thread that never turns
it would be a hang.

### 3. `js_notify_main_thread` is a broadcast

Before P9 only the primary agent could be inside a turn, so "wake the route" and
"wake everyone parked" were the same thing. They are not any more, and the
difference is a hang: a Worker now parks in its own turn instead of on
`PUMP.cvar`, so a point-to-point wake addressed to the primary agent would leave
it asleep on a `postMessage`-driven resolution.

A broadcast is also what it replaces. `event_pump::NOTIFIED` is one
process-global flag that every JS thread consumes with a `swap`, and the legacy
park's condvar is signalled for whoever waits on it — the wake has never been
addressed. Only agents actually inside a turn are poked, so an idle agent costs
nothing and a single-agent program behaves exactly as before.

The ordering is the same handshake P0 documented, with the count standing in for
the single flag: the owner stores its route's `in_turn`, then increments
`PARKED_LOOPS`, then re-reads `NOTIFIED`; a producer stores `NOTIFIED` and then
loads `PARKED_LOOPS`, both `SeqCst`. In the single total order either the
producer sees the increment (and therefore the flag, which precedes it) or the
owner sees the store. There is a test that parks a worker agent and asserts a
`js_notify_main_thread` from another thread ends its turn.

### 4. A worker agent's loop is torn down at `retire_agent`, not at thread exit

`agent_loop::shutdown_current_thread()` had exactly two non-test callers, both
on the primary thread: the process-exit funnel in `gc/mod.rs` and
`js_unsettled_top_level_await_exit`. A worker agent's loop would have been
destroyed only by `AgentLoop::drop` at thread exit, which skips the settle
sequence entirely.

That sequence is not hygiene. It closes the handles this agent still owns and
runs one nonblocking turn so their terminal completions reach the binding
(exactly-once release, DESIGN D4) — and P5, P6 and P7's engines learn about
teardown **only** through those completions; none of them registers a teardown
hook of its own. A worker agent that skipped it would strand every promise those
engines owe, which presents as a hang rather than an error. P4's pool has the
same rule for jobs: a job still running when the loop goes away completes into a
closed `WorkPort` and is silently discarded.

So `agent::retire_agent` runs it first, while the arena is still mapped, and
before the timer and thread-result purges — because a completion delivered by
that turn can legitimately queue a timer or a thread result, which is what those
purges are there to drop.

### 5. Per-thread handle ids are banded by agent

`turnloop_proc` and `turnloop_pool` mint their ids from a per-thread counter
starting at 1. That was sound while one thread could own a loop: there was one
minter. With N agents, two agents both own an id `1` — harmless while every
lookup is same-thread (each finds its own entry) and a **silent misroute** the
moment one is not.

Each agent now mints from `(agent & 0xFFFF) << 40`, leaving it 2^40 ids inside
the 56-bit token field. A foreign id misses the table instead of aliasing
another agent's entry, so the failure is an error the caller can see rather than
a completion delivered to the wrong subscriber. The primary agent's band is 0,
so its ids are unchanged and nothing about a single-agent program moves. There
is a test that mints on three agents and asserts disjointness, because the
property is invisible in normal operation — it only shows up the one time
something crosses.

`turnloop_net` needed no equivalent: its ids come from the *binding*, and
`perry-ext-net`'s come from a process-global domain
(`perry_ffi::reserve_handle_id_in_domain`), while `perry-db-turnloop` already
uses a process-wide `AtomicI64` for the same reason. Its per-thread `NET` map
is documented "per agent, like the loop itself".

## What did NOT move, and why

This lane changed an admission predicate. It removed no tokio edge, and the
`scripts/tokio_inventory.py` gate still counts **46 manifest edges across 16
workspace crates** — unchanged, deliberately. What changed is the *condition*
under which each is reached, which is why eighteen `reached_when` / `blocker`
strings in `scripts/tokio_inventory.json` were rewritten: "a worker_threads
agent (no loop: `agent_loop::net_available` is `current_agent() ==
PRIMARY_AGENT`)" is no longer true of any of them.

Every remaining decline, by cause:

| still declines | cause | who closes it |
|---|---|---|
| any surface, in the `tokio-wait-driver` A/B arm | there is no loop at all, by construction | nobody — it is the baseline |
| ~~any surface, when `Loop::new` fails~~ — **no longer a decline**: it aborts (`agent_loop::loop_creation_failed`) | descriptor exhaustion. "An unsupported host" is not a runtime cause: turnloop has no no-op backend, so such a target fails to compile | closed — the fallback pinned one thread to tokio for its whole life and said so only under `PERRY_LOOP_STATS` |
| any surface, on a second thread acting for an agent another thread owns | exactly one thread owns an agent's loop (Android's UI pump) | nobody — it is the rule, and it preserves Android |
| `net`/`tls` after `socket.upgradeToTLS` | P1 kept the tokio socket so the TLS upgrade keeps working | a `turnloop-tls` client path |
| `http.createServer` in a **cluster worker** | the `SO_REUSEPORT` bind is not reachable through `ListenOpts` | PerryTS/turnloop#49 |
| `http.createServer` with an attached `WebSocketServer` | tungstenite 0.29 stored types vs `turnloop-websocket`'s 0.30 | P8 group **E** |
| `node:http`/`node:https` **client**, HTTP/2 | never migrated | P8 groups **C** and **D** |
| all four databases with TLS, a UDS host, or a non-trivial topology | `perry_db_turnloop` has no TLS, no `pipe_connect`, no SRV/SDAM | P8 group **B** |
| `new Redis()` in its DEFAULT configuration | `REDIS_TLS` defaults to `true` and the binding declines unless it is the literal string `false` (P7 defect 6, perry#10335) | P8 group **B** |
| `fetch` through a proxy, `axios`, `node-fetch` | never migrated | P8 group **G** |
| `perry-stdlib`'s own `tokio` | the promise bridge every `perry-ext-*` crate settles through | P8 group **L**, last by construction |

So the honest statement about group **A** is: **its precondition is now met, and
its four edges are not yet removable.** `perry-ext-net`'s two need the TLS client
path as well (the `upgradeToTLS` row above); `perry-ext-http`'s `hyper` +
`hyper-util` need this lane **and** group E **and** turnloop#49 together,
exactly as P8 costed it. Removing an edge means deleting its line from
`tokio_inventory.json`, and there is no line this lane can honestly delete.

### `perry/thread` agents have no event loop to give a loop to

The brief's acceptance case names two kinds of non-primary agent. The
`worker_threads` Worker is the one a Node program uses, and it is covered below.
The other — `perry/thread`'s `spawn`, `parallelMap`, `parallelFilter` — turns
out not to be a surface at all, and this is worth stating because "P9 did not
reach it" and "there is nothing there to reach" look identical in a green suite.

All three thread bodies live in `crates/perry-runtime/src/thread.rs`
(`:1136`, `:1392`, `:1600`). Each calls the user closure **once, synchronously**,
serializes its return value and exits. There is no microtask pump, no timer
tick and no `js_wait_for_event` anywhere in them, so such an agent never parks
and never creates a loop — before this lane or after it. An `async` closure
returns a Promise, which is not a value that can cross an agent boundary.

That is also the answer to the 64-core cost question below: a `parallelMap` over
64 cores creates **zero** loops, because none of its workers ever parks.

## What an agent loop costs

A `turnloop::Loop` preallocates at `Loop::new` and the config cannot grow in
place (PerryTS/turnloop#43), so the size is chosen once. There are two
profiles and the choice between them is what bounds the cost:

| profile | when | `max_handles` | `max_operations` | pooled buffers | `post_capacity` |
|---|---|---|---|---|---|
| `Wait` | the agent parks but submits nothing | 16 | 16 | 0 | 16 |
| `Net` | the agent's first net or pool submission | 4096 | 8192 | 64 x 16 KiB = 1 MiB | 256 |

`ensure_loop()` — the park path — asks for `Wait`. Only a submission upgrades to
`Net`, and the upgrade is a recreate that is asserted to happen while the loop
owns no handles and no outstanding pool job. So the agents that pay the net
profile are exactly the agents that do network I/O.

### Measured

`scripts/turnloop/apps/p9_agent_loop_rss.ts`, on the Linux box, three modes x
three agent counts x three repetitions per arm, median reported. A row's
`delta_kb` is process RSS once every agent is alive and idle, minus RSS before
the first Worker was created, taken after a 1500 ms settle (see below).
`loop_lines` counts `[perry-loop] driver=turnloop` lines, which is the direct
assertion of how many agents actually owned a loop.

| agents | mode | base `1edb5b7e8d` | this branch | delta | base loops | branch loops |
|---:|---|---:|---:|---:|---:|---:|
| 1 | idle | 10,096 KB | 9,972 KB | **-124 KB** | 1 | 1 |
| 8 | idle | 20,332 KB | 21,040 KB | **+708 KB** | 1 | 1 |
| 64 | idle | 498,520 KB | 494,300 KB | **-4,220 KB** | 1 | 1 |
| 1 | net | 29,192 KB | 13,316 KB | **-15,876 KB** | 1 | 2 |
| 8 | net | 172,900 KB | 23,548 KB | **-149,352 KB** | 1 | 9 |
| 64 | net | 514,280 KB | 716,172 KB | **+201,892 KB** | 1 | 65 |

Four things to read out of it.

**An idle agent loop costs nothing, because there is not one.** The three `idle`
rows are the same on both arms to within a megabyte across the whole process,
and `loop_lines` is 1 on both -- a Worker that never submits produces no
`[perry-loop]` line at all, at 1 agent or at 64. That is the `Wait`/`Net` profile
split doing its job: an agent pays when it does I/O, not when it exists.

**`loop_lines` is the subject assertion, and it is unambiguous.** In `net` mode
this branch prints exactly `agents + 1` lines -- 2, 9, 65 -- and the base commit
prints 1 whatever the agent count. Sixty-four worker agents each owned their own
`turnloop::Loop`; on the base commit none of them did. A table of RSS numbers
without such a count could not tell those two situations apart, which is the
failure mode this project has paid for before.

**A net-profile agent loop costs about 3.4 MB.** Taking `net - idle` within this
branch removes the Worker itself from the number: 1 agent gives +3,344 KB and 64
agents give +221,872 KB, which is +3,467 KB per agent -- two independent agent
counts agreeing to 4 %. The base-arm comparison at 64 agents says the same thing
from the other side: +201,892 KB for 64 loops is +3,155 KB each. That is the
right order for what `net_config()` preallocates (64 x 16 KiB pooled buffers is
1 MiB of it; the 4096-handle and 8192-operation tables are the rest).

**The 8-agent `net` row does not fit, in EITHER arm, and is reported rather than
smoothed.** This branch spends only 2,508 KB more than its own idle row for eight
loops, where one loop costs 3,344 KB; the base arm spends 152,568 KB more than
its idle row for zero extra loops. Both are stable across their three
repetitions, so neither is one bad sample. The likeliest explanation is collector
timing -- the settle window lets a collection land, and whether it does depends on
how much the arm allocated -- but that is a hypothesis, not a measurement, and the
conclusion above rests on the 1-agent and 64-agent rows.

**Why this branch is CHEAPER at 1 and 8 net agents.** Not a rounding artefact: a
Worker's `fetch` on the base commit spins up the reqwest client stack on that
agent, and on this branch it uses turnloop's, which is much smaller. At 64 agents
the loop preallocation overtakes that saving; below that it does not.

**A `parallelMap` over 64 cores creates ZERO loops**, which is the other half of
the cost question and is measured rather than argued:
`scripts/turnloop/apps/p9_thread_agent.ts` runs `spawn` and `parallelMap` and
prints exactly one `[perry-loop]` line, `agent=0`. See the `perry/thread` section
above for why there is nothing there to give a loop to.

### A measurement note that changed the answer

The first version of this table used `postMessage` for readiness and
`worker.terminate()` for teardown, and read RSS the moment the last worker
reported. It was wrong twice, and both mistakes moved the answer.

The runner took `$?` after a `| head -1`, so a run killed by `timeout` reported
exit 0: every row in the first table came from a process that had hung, and the
rows that appeared at all were the ones whose stdout happened to be flushed
first. And with no settle window the numbers were two to six times larger
(27,892 KB for the 1-agent idle row, against 9,972 KB settled), because they
included transient allocation the collector had not reclaimed yet -- a per-agent
cost that FALLS as agents rise is not a preallocation, it is a reading taken too
early.

The probe now signals readiness through the filesystem and leaves by
`process.exit`, and prints its own `settle_ms` in every row so a table cannot be
read without knowing which settle produced it.

### The sizing decision, and why `net_config()` is unchanged

`net_config()` was chosen for a server: a listener, its connections, and their
in-flight reads and writes. The obvious saving would be a smaller profile for
non-primary agents — a Worker doing three client sockets does not need 4096
handles or 64 pooled buffers.

It is not taken here, for two reasons.

First, a Worker **can** be a server. `worker_threads` is how a single-process
worker-per-core HTTP server is written, and a smaller worker profile would give
that server a connection cliff that the primary agent does not have — while
`max_handles: 4096` is *already* the cause of perry#10351 (a turnloop server
refuses the 2,049th connection, because a connection costs two handles). Making
the same constant smaller somewhere else, in the same change that first lets a
Worker reach it, is the wrong order.

Second, because the config cannot grow in place, a wrong small choice is not
recoverable: a Client-sized agent that later listens cannot be upgraded, since
the upgrade requires the loop to own no handles and a client socket is a handle.
Sizing down therefore needs either turnloop#43 or a role signal at creation,
and neither is this lane.

What this lane does instead is make the cost *conditional*: the profile is per
agent, the default is `Wait`, and an agent that never submits never pays. The
measured numbers above are what the decision should be revisited against.

## Instruments: what a number means now, and what it does not

This lane touches the A/B's own measuring equipment, so the changes are named
explicitly rather than left for the harness to discover.

**The `[perry-loop] driver=turnloop …` marker line keeps its exact field
order.** `agent=<id>` is *appended*, never inserted. Two instruments parse it
positionally — `scripts/turnloop/server_ab.py` matches the literal prefix
`[perry-loop] driver=turnloop` as its arm marker and rejects any sample whose
marker is missing or wrong, and `scripts/turnloop_p0_loop_stats.py` has a regex
anchored on `driver=turnloop turns=… turn_errors=…`. A field in the middle would
have made the A/B harness reject every sample as "wrong arm", which is the
check that stops it comparing a tree against itself. The `driver=legacy` and
`driver=turnloop parked=0` lines keep their prefixes for the same reason.

**There is now one `[perry-loop]` line per agent that owned a loop, not one per
process.** This is the definition that moved. On a single-agent program —
including `scripts/turnloop/apps/node_http_hello.ts`, which is what the server
A/B measures — nothing changes: one line, `agent=0`. On a program with a Worker
there are two or more, and because a worker retires *during* the program while
the primary retires at exit, **the worker's line comes first**. `server_ab.py`
picked the first matching line, so it would have described its sample with a
worker's counters; `pick_marker()` now prefers the `agent=0` line and falls back
to the first match, which is exactly the old behaviour on any build that
predates this lane (no `agent=` field, one match).

Neither of the harness's two rejection checks is weakened by that, and one is
strengthened. `verify_marker` still refuses an arm whose marker substring is
absent, and whose `[perry-loop-waits]` line is missing or names the wrong arm,
*before* `pick_marker` is consulted at all -- and it now raises rather than
logging if selection somehow returns nothing, because a `next(...)` that used to
raise becoming a function that returns `None` is precisely how a verification
decays into a log line. `finish_sample`'s per-sample check is unchanged in form
and stricter in effect: a program whose PRIMARY agent fell back to the legacy
driver while a Worker ran on turnloop is now described by the primary's
`driver=legacy` line and correctly marked invalid, where taking the first match
would have let the Worker's `driver=turnloop` line stand in for it.

**The process-wide lines still print exactly once.** `p2 adopted=`, `p4
pool_submitted=` and P6's `http_submitted=` are lifetime totals for the whole
process, so `print_stats` emits them only under the primary agent's line, whose
shutdown is the process-exit funnel and therefore the last to run. A worker
retiring mid-program would otherwise print a partial copy of each.

**`[perry-loop-waits]` is still a PRIMARY-AGENT-ONLY measurement, and was
deliberately left that way.** `loop_stats::recording_thread()` is
`current_agent() == PRIMARY_AGENT` because the wake-latency histogram is built
on two process-global slots (`PARKED`, `NOTIFY_AT_NS`) that assume one parked
recorder; a second recording thread would invent latencies rather than add
samples. So in that line:

* `turnloop_waits=` / `condvar_waits=` count the primary agent's parks only;
* **`tokio_ticks=0` means "the primary agent took no tokio tick", not "no agent
  did"**.

The per-agent statement is the `native_ticks=` field of that agent's own
`[perry-loop]` line, which is what this lane's acceptance reads. Making the
histogram per-agent would change what the published A/B numbers mean, so it is
named as a gap rather than done here.


## The gap suite, both arms

Both trees built from source in their own checkouts on the Linux box, the same
cargo invocation and the same `-p` set in each (`perry`, the two `-static`
wrappers, and the nine `perry-ext-*` wrappers), `npm ci` in both, Node 26.5.1 on
PATH, run as `PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh`. Baseline is this
branch's own base commit, `1edb5b7e8d`.

| | base `1edb5b7e8d` | this branch |
|---|---:|---:|
| parity pass | 809 | 809 |
| parity fail | 10 | 10 |
| compile fail | 0 | 0 |
| crashed | 0 | 0 |
| total | 819 | 819 |
| parity rate | 98.7 % | 98.7 % |

**Per test, the two failure sets are identical. Zero status changes.** Named in
full, because a count is not a comparison:

```
test_gap_2159_defineproperty_class_prototype
test_gap_2514_settracesigint
test_gap_2899_2779_2777_static_helpers          (already red on base per the brief)
test_gap_disposablestack_2875                   (already red on base per the brief)
test_gap_iterator_prototype_next_patch          (already red on base per the brief)
test_gap_json_lazy_defineproperty_index
test_gap_perfhooks_3088_3008_3010_3011
test_gap_prop_plan_cache_invalidation
test_gap_turnloop_p9_worker_agent_net           (this lane's new test -- see below)
test_gap_v8_2_3680plus
```

### The new test, and why it was red in that sweep

`test_gap_turnloop_p9_worker_agent_net` is added by this lane, and in the sweep
above it failed on **both** arms with `WORKER NEVER ANSWERED`. That is a defect
in the test, and the fix is instructive enough to record.

It had three cases and posted all three results in one message at the end. Cases
1 and 2 -- a `fetch` from a Worker, and the same `fetch` after that Worker has
parked on a timer, which is the shape P8 measured as a hang -- both SUCCEEDED on
this branch; the standalone run says so directly (`p6 http_submitted=2
declined=0 completed=2`). Case 3 hung, and took the two passing results with it,
so the run reported a Worker that had answered twice as one that never answered
at all. This is the same batch-reporting mistake the acceptance probe had, found
twice in one lane.

The Worker now posts each case as it finishes, and case 3 -- a raw `net.connect`
from a worker agent to a listener owned by the same process's primary agent --
is removed from the gap test and kept as a named reproducer instead, because it
is a defect of its own and a gap test asserting it would be a test of that defect
rather than of this lane. With that change, verified directly:

```
this branch : immediate status=200 body=hello/one     exit 0, byte-identical to node
              after-timer status=200 body=hello/two
              done
base commit : immediate status=0 body=undefined       exit 1, PARITY_FAIL
              after-timer status=0 body=undefined
              done
```

Through the harness itself, not just standalone -- so normalization and the
oracle comparison are the ones the suite would apply:

```
$ PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_turnloop_p9_worker_agent_net
this branch : PASS           Parity Pass: 1  Parity Fail: 0
base commit : PARITY_FAIL    Parity Pass: 0  Parity Fail: 1
              Node.js:  immediate status=200 body=hello/one
              Perry:    immediate status=0 body=undefined
```

**The sweep table above predates that fix**: it was run against the three-case
version, and the failure set it reports for both arms includes this test for a
reason that no longer applies. On this branch the suite therefore stands at 810
pass / 9 fail; the base arm stays at 809 / 10, because the test genuinely fails
there. Every OTHER test's status is unchanged between the arms, which is the
claim that matters, and it was measured over the full 819 in both.

## GC stress

An agent loop holds JS promises across completions, so this is the lane where a
correct-looking answer over a corrupt heap is most likely.
`scripts/turnloop/apps/p9_worker_gc_stress.ts` runs the primary agent and a
Worker through a `fetch`, a socket round-trip and an allocating churn loop at the
same time, retains a graph across all of it, and reads that graph back after
every collection point.

Three unstressed reference runs first, all byte-identical to each other. Then
four seeds:

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<n> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_GC_FROMSPACE_SCAN_ABORT=1 \
PERRY_LOOP_STATS=1 ./p9_worker_gc_stress
```

| seed | exit | stdout vs reference | safepoints | forced collections | copying minors | objects MOVED | from-space scans | dangling |
|---:|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | 0 | byte-identical | 28,186 | 28,186 | 28,186 | 20,354 | 56,372 | 0 |
| 7 | 0 | byte-identical | 28,186 | 28,186 | 28,186 | 20,358 | 56,372 | 0 |
| 424242 | 0 | byte-identical | 28,186 | 28,186 | 28,186 | 20,350 | 56,372 | 0 |
| 987654321 | 0 | byte-identical | 28,186 | 28,186 | 28,186 | 20,354 | 56,372 | 0 |

**Byte-identical stdout is not the verdict, and was not treated as one.** The
brief is explicit that a probe retaining an object graph across collections can
validate its own output and still be measuring a corrupt heap -- the same binary
printed a byte-identical correct answer carrying 15,018 dangling references
before an unrelated fix and 0 after. The columns that carry the verdict here are
the last two, from `PERRY_GC_FROMSPACE_SCAN_ABORT=1` (which implies
`PERRY_GC_FROMSPACE_SCAN=1`; alone it used to be inert and report success):
**56,372 from-space scans per run, every one `[gc-fromspace-scan clean]`, with
`missing_rewrites=0 dangling=0 owners=0`.** An abort would have ended the run.

Three further checks that the instruments were armed rather than merely
requested, because a stress arm that armed nothing is a pass that proves nothing:

* `[gc-schedule] seeded GC-schedule fuzzing ACTIVE: seed=1 rate=1` printed at
  startup, and `loop_polls=28142` in the exit verdict -- the back-edge polls that
  make the seeded schedule reachable inside a loop were emitted.
* `moved_objects=20354` -- the collector really evacuated; a run with
  `copying_minors>0` and `moved_objects=0` would have proved nothing about
  rewriting.
* `[gc-fromspace-protect] mode=ProtectPages ... sets_held=N/800` -- the
  quarantine was live at the depth asked for, with pages `mprotect`ed rather than
  only poisoned.

The four seeds give identical safepoint counts on purpose: at
`PERRY_GC_SCHEDULE_RATE=1` every handled safepoint is selected whatever it
hashes to, so the seed stops mattering. That is the maximum-density endpoint,
which is what this lane wants; it is stated here so the identical counts are not
read as the seed having been ignored. `moved_objects` still varies slightly
between them (20,350-20,358), which is the residual timing.

The worker agent's own loop ran throughout: `[perry-loop] ... native_ticks=0 ...
completions=13 agent=1`, with `p6 http_submitted=2 declined=0`.

## Unit tests

`RUST_TEST_THREADS=1 cargo test --release -p perry-runtime` (single-threaded
because that crate's tests share process-global side tables, #1444):
**4018 passed, 1 failed, 4 ignored.**

The one failure is `gc::tests::heap_generation::a_free_or_move_outside_every_scope_is_caught_in_debug_builds`,
and it is an artefact of running the suite in `--release`, not of this branch.
The test asserts that `gc::heap_generation::debug_assert_heap_change_open()`
panics with no scope open; that function's body is `#[cfg(debug_assertions)]`
(`crates/perry-runtime/src/gc/heap_generation.rs:126-134`), so in a release build
it is a no-op and nothing can fire. This branch touches no file under
`crates/perry-runtime/src/gc/` at all -- `git diff 1edb5b7e8d..HEAD -- crates/perry-runtime/src/gc/`
is empty.

The fifteen tests this lane added or rewrote all pass, and they are the ones that
assert the properties the design rests on:

```
a_worker_agent_gets_its_own_loop                         ... ok
sibling_worker_agents_do_not_share_a_loop                ... ok
a_second_thread_of_the_same_agent_is_declined            ... ok
a_notify_wakes_a_parked_worker_agent                     ... ok
another_thread_wakes_a_parked_turn_through_js_notify_main_thread ... ok
install_shutdown_and_thread_exit_release_the_loop_and_route      ... ok
native_work_in_flight_is_counted_as_a_tokio_tick_not_a_turn      ... ok
an_armed_timer_deadline_does_not_keep_the_loop_alive     ... ok
```

plus the `turnloop_proc` id-banding test, which mints on three agents and asserts
the bands are disjoint -- a property that is invisible in normal operation and
only shows up the one time an id crosses.

## Defects found and NOT fixed

Each of these is reproducible, has a named fixture on this branch, and is left
open deliberately. None is a regression: every one of them is either broken on
the base commit too, or is a surface that did not exist there.

### 1. A raw socket between agents in one process

A `net.connect` from a worker agent to a `net.createServer` listener owned by
the same process's PRIMARY agent connects and then never receives data.
Reproducer: `scripts/turnloop/apps/p9_worker_socket_to_primary.ts`.

```
this branch, 1 agent : answered=1/1 ok=1/1
this branch, 2 agents: answered=2/2 ok=1/2   (ok, error:timeout)
base commit          : no output at all, exit 1
```

A Worker socket to an OUT-OF-PROCESS server works on this branch and is covered
by the acceptance probe; on the base commit it returns `echo=""`, i.e. this shape
has never worked on either transport. What this lane changed is that it now
reaches turnloop instead of failing inside tokio.

### 2. Concurrent worker agents each holding a raw socket

`scripts/turnloop/apps/p9_socket_concurrency.ts` runs the SAME N socket
round-trips two ways, which is what makes it a diagnosis rather than an
observation:

| N | `P9_MODE=primary` (1 agent, N sockets) | `P9_MODE=agents` (N agents, 1 socket each) |
|---:|---|---|
| 1 | ok=1/1 | ok=1/1 |
| 2 | ok=2/2 | ok=2/2 |
| 4 | ok=4/4 | ok=2/4 -- `error:no-data`, `error:write ENOENT` |
| 8 | ok=8/8 | hang |
| 16 | ok=16/16 | hang |

So it is not socket concurrency: one agent holds sixteen at once without a
wobble. It is crossing AGENTS, which is this lane's surface. `ENOENT` on a WRITE
is `turnloop_net`'s "this id is not in my thread-local table", and the hang is
its consequence -- a write that was never submitted means data that never
arrives, so every agent parks forever. `gdb` on a wedged 4-agent run shows all
five threads (primary plus four workers) blocked in `epoll_wait` with no pending
wake, including the primary, whose own watchdog timer therefore never fires.

It is NOT fixed here, but it is traced to a named structure rather than left as
a symptom. `perry-ext-net`'s socket state is split across a THREAD-LOCAL map and
several PROCESS-GLOBAL ones, and P9 is what first makes more than one thread
populate the thread-local side:

* `turnloop_net::NET` (`crates/perry-runtime/src/turnloop_net/mod.rs:218-228`) is
  thread-local, keyed by the socket id, and holds the loop `Handle`.
* `statics::sockets()` (`crates/perry-ext-net/src/lib.rs:159-162`) is
  process-global and holds `SocketState.turnloop`, the routing flag decided once
  at creation (`lib.rs:1296-1320`).
* `statics::pending_events()` (`crates/perry-ext-net/src/lib.rs:184-187`) is
  **one process-wide queue**. Sinks on any agent's thread push into it
  (`push_event`, `lib.rs:506-518`) and it is drained by whoever calls
  `js_ext_net_drain_pending`.

Two consequences follow directly, and between them they account for both
symptoms:

1. `SocketState::command` (`lib.rs:311`) routes to the turnloop path purely on
   the stored `turnloop` flag, so a thread that is not the socket's owner
   submits into a `NET` map that does not contain the id and gets
   `not_found("write")` -- **the observed `error:write ENOENT`**
   (`turnloop_net/mod.rs:528`).
2. A socket's `'connect'`/`'data'`/`'close'` events are queued process-wide, so
   an agent can drain an event belonging to another agent's socket, whose JS
   listener lives in a different heap. The owner then never sees the event, its
   promise never settles, and it parks forever -- **the observed hang**, which
   the runtime itself reports as `Warning: Detected unsettled top-level await`.

The matching predicate is asymmetric in the other direction too:
`turnloop_net::is_live` (`mod.rs:631-636`) consults only the calling thread's
`NET`, so called off-owner it answers `false` and silently falls through to the
tokio branch even though `SocketState.turnloop` is `true` (used as a transport
test at `perry-ext-net/src/lib.rs:1031`).

The fix is to give the pending-event queue the same ownership the loop now has
-- route each event to its socket's OWNING agent and let each agent drain only
its own -- and to make the off-owner command path a loud error rather than an
ENOENT. That is a `perry-ext-net` change with its own validation cost (the tokio
path pushes into the same queue from tokio worker threads, so it cannot simply
become thread-local), which is why it is not folded into this lane.

**Stated honestly: this is a diagnosis, not a confirmation.** The structures
above are read from the source and they fit every observation, but no
instrumented run has yet caught an event being drained by the wrong agent. The
probe was extended to check that a worker receives its OWN payload rather than
merely some payload (`error:CROSSED-AGENT`), and in four further runs it never
fired -- every failure was an event that arrived nowhere, not one that arrived in
the wrong place. Whoever takes this should confirm it with a counter on the drain
path before changing anything.

### 3. `worker.terminate()` on a Worker parked waiting for a message

Both arms. It never returns, so the process hangs after doing all its work. It
is what made the first RSS table unreadable. Not reproduced by the minimal
fan-in probe (`p9_worker_message_fanin.ts` terminates cleanly at 1/2/4/8 agents
across three keep-alive shapes), so the trigger is narrower than "terminate a
parked Worker" and I could not isolate it further. The RSS probe routes around it
by leaving via `process.exit`.

## What I did NOT do

* **No benchmarking.** No timing number anywhere in this report. The box is
  shared and was running two gap sweeps and another lane's work throughout; RSS
  and counters are load-independent, wall time is not. The tokio-vs-turnloop A/B
  belongs to the coordinator on the quiet mini.
* **Nothing ran on macOS or Windows.** Every result here is Linux
  (perrybuilder, EPYC 9354P). The RSS probe reads `/proc/self/status` and says so
  when it cannot.
* **No tokio edge was removed.** `scripts/tokio_inventory.py` still counts 46
  manifest edges across 16 crates, deliberately -- see "What did NOT move".
* **`net_config()` is unchanged**, and perry#10351 (a turnloop server refuses the
  2,049th connection because a connection costs two handles) is untouched. The
  measured 3.4 MB per net-profile loop is what a future decision to shrink it
  should be weighed against.
* **The per-agent socket defect above is diagnosed, not fixed.**
* **`[perry-loop-waits]` was not made per-agent.** It remains a primary-agent-only
  measurement; making it per-agent would change what the published A/B numbers
  mean.

## turnloop gaps found

For the coordinator to file on PerryTS/turnloop; not filed from here.

1. **`Config` cannot grow in place** (already PerryTS/turnloop#43). This lane
   worked around it with two fixed profiles and an upgrade that recreates the
   loop while it provably owns no handle and no outstanding pool job. The cost of
   the workaround is that an agent's size is decided before its role is known, so
   a Worker that will only ever make three client sockets still gets a
   server-sized loop the moment it makes the first one. A growable `Config`, or a
   role hint at `Loop::new`, would let the net profile be chosen by what the
   agent does rather than by what it might do.
2. **No per-loop accounting of preallocated bytes.** The 3.4 MB figure in this
   report is inferred by differencing process RSS across three modes and three
   agent counts, with one row that does not fit. A `Loop::reserved_bytes()` -- or
   the same number on `Config` -- would make it a read instead of an experiment,
   and would let a program with 64 agents budget before it allocates rather than
   after.
