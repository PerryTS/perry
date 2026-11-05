# turnloop P2 — processes, pipes, datagrams and signals

Branch `turnloop/p2-process`, based on `turnloop/p1-net` at `c6f185d6e8`.
Built and tested on the shared Linux box (EPYC, nightly-2026-08-20, LLVM 22)
against the pinned gap oracle Node **26.5.1**. Nothing here was run on Windows,
and the Windows arms are named in "For the integrator".

P2's subject is the list DESIGN §5a calls "many ad-hoc threads": every thread
whose only job was to turn a blocking syscall into a queue push plus a
`js_notify_main_thread()`. This phase moves four of them and explains, with
the specific missing API, why the rest did not move.

## What moved, and what did not

| subsystem | threads before, per resource | after P2 | why |
|---|---|---|---|
| `node:dgram` socket | 1 (`recv_from` with a 250 ms poll timeout) | **turnloop**: receive and send are operations | self-contained; the socket is Perry's and stays Perry's |
| child stdout / stderr / extra `stdio` fd | 1 per readable pipe | **turnloop**: one multishot read each | one call seam (`cp_spawn_reader`) |
| OS signals (INT, TERM, HUP, USR1, USR2) | 1 process-wide `perry-signal-wake` thread + a self-pipe | **turnloop**: a loop subscription per signal | turnloop's dispatcher has a portable name for each |
| OS signals (QUIT, ABRT, BUS, PIPE) | the same thread | `sigaction` + self-pipe, started **only** when one of these is subscribed | turnloop 0.1 has no portable name; ABRT/BUS are co-owned by the GC quarantine reporter |
| child exit (`waitpid`) | 1 per child | thread | see below |
| child IPC channel (`fork`) | 1 per forked child | thread | see below |
| child stdin drain | 1 per child, **lazily** started on a backed-up pipe | thread | see below |
| pty master read + waiter | 2 per pty | thread | see below |
| `process.stdin` | 1 process-wide | thread | see below |
| `process.stdout` / `stderr` | **none** | unchanged | they never had one |

`process.stdout.write` and `process.stderr.write` are synchronous
`write_all` + `flush` on the calling thread, with the completion callback on
`nextTick` (`os_process_streams.rs`). There is no thread and no tokio there, so
there is nothing for P2 to delete; moving them to the loop would change
`write()`'s return value and its ordering against `process.exit()`, which is a
behaviour change, not a migration. They are listed here because "stdout/stderr
adapters" is in the phase's scope and the honest answer is that the scope item
is already satisfied.

## Why the child *spawn* did not move

turnloop has a complete process API — `Loop::spawn(&ProcessSpec)`, `kill`,
`kill_group`, and an `Exited` completion carrying `ExitStatus`. It is not
usable for Perry's `child_process` yet, for two concrete reasons:

1. **`ProcessSpec.stdio` is `[ProcessStdio; 3]`.** Perry's `fork()` passes the
   IPC socket to the child as **fd 3** with `NODE_CHANNEL_FD=3` (`fork.rs:207`,
   and a `pre_exec` that dups it into place), which is the convention a Node
   child reads; and `spawn`'s `stdio` option accepts arbitrary extra
   descriptors, which `options.rs` implements with further `pre_exec` dups.
   Neither has an equivalent in `ProcessSpec`.
2. **There is no `pre_exec` hook.** Beyond the fd maps, Perry uses it for
   `detached`'s `setsid` and for its uid/gid ordering.

So the spawn itself stays on `std::process::Command`, and with it the exit
wait: turnloop reports an exit only for a process **it** spawned, and 0.1 has
no way to adopt a pid. The per-child waiter thread therefore survives.

The alternative — `signal_start(Signal::Chld)` plus `try_wait()` per registered
child, which is what libuv does — was considered and rejected for this phase.
It is a correct design, but it changes who reaps, and Perry has three other
users of `waitpid` in the same process (`spawnSync`, `execSync`, the pty
waiter). Getting that wrong steals another caller's exit status, which is
exactly the class of bug that shows up as an unrelated flake weeks later. It
belongs in its own change with its own test, not bundled into a phase that is
already moving four subsystems.

**What unblocks it**, either one sufficient:

- `ProcessSpec` gaining extra child descriptors (`stdio: Vec<ProcessStdio>`, or
  an explicit `extra_fds: Vec<(RawFd, Handle)>`) plus a `setsid`/session
  option, after which the whole spawn moves and `Exited` replaces the waiter;
- or `Loop::adopt_process(pid) -> Handle`, which would let the exit wait move
  on its own while the spawn stays where it is.

## Why the other threads did not move

- **Child IPC (`fork`)**: the reader thread owns the parent end and
  `child.send()` writes through a `try_clone()` of it. `dup(2)` shares the open
  file description, so adopting either copy makes the *other* non-blocking, and
  `child.send()`'s `write_all` would start failing with `EWOULDBLOCK`. Moving
  the reads therefore forces moving the writes in the same change — which means
  reshaping `child.send()`'s synchronous boolean return, the V8 advanced
  framing accumulator, and `disconnect()`'s ordering. Contained, but a separate
  change.
- **Child stdin drain**: the thread is *lazy* — it starts only when a write
  exceeds pipe capacity (#9493's `uv_try_write` shape), so it is not a
  per-child cost. Moving it means the loop holds a duplicate of the write end,
  and the child then only sees EOF when **both** copies close; getting the
  `end()`-with-queued-bytes ordering wrong hangs every `child.stdin.end()`.
  High blast radius, small gain, so not in this phase.
- **pty**: feasible and worth doing next. `Detached::from_fd` classifies a pty
  master as a stream on both hosts (`isatty` is true for a master on Linux and
  macOS — verified, because the obvious guess is that it is not), so the reader
  thread can move exactly as the child pipes did. It is left out here only
  because it needs a pty-driven acceptance test to be worth claiming, and the
  same `dup`/`O_NONBLOCK` argument as IPC applies to `write_pty`.
- **`process.stdin`**: the reader thread is shared with perry-stdlib's readline,
  which owns fd 0 in some configurations (#9692, #9676, #9594 are all
  regressions in exactly that overlap). Moving it needs those three
  PTY-driven integration tests as the gate, which is more validation than this
  phase can carry alongside four other subsystems.

## Architecture

`crates/perry-runtime/src/turnloop_proc/`.

- `mod.rs` — the loop-owned entry table: adoption, multishot reads, datagram
  send/receive with rearm, signal subscriptions, ref/unref, exactly-once close,
  and the completion translation.
- `adopt.rs` — `Detached::from_fd` / `from_socket` / `from_handle`, plus the
  descriptor duplication a subsystem needs when it must keep a copy.
- `registry.rs` — `Owner` and `StreamEvent`: who a completion belongs to and
  what it looks like when it gets there.

**No C ABI.** P1 needed one because `perry-ext-net` is a separately linked
`staticlib`; every P2 subsystem is compiled into perry-runtime, so a completion
reaches its owner through an enum and a `match`, and the compiler checks that
every owner handles every event it can receive. That is the whole reason this
is a second module rather than a fifth subsystem slot in P1's sink registry.

**One token space, disjoint from P1's by construction.** The top 8 bits are the
operation class — `0x10`–`0x1F` here against P1's `1`–`7` — and the low 56 are
the Perry-side id. `agent_loop::dispatch_staged` routes on exactly that range
test, so neither module can be handed the other's completion, and a stale token
from a closed handle finds no entry and is dropped. `the_two_token_spaces_do_not_overlap`
tests the routing contract directly rather than inferring it from a passing
workload.

### Adoption, not re-implementation

P2 does not re-create the descriptors it moves. A dgram socket carries Node's
bind-time `SO_REUSEADDR` / `SO_REUSEPORT` / `IPV6_V6ONLY` decisions and,
afterwards, its multicast membership, interface and TTL state; a child's pipes
come out of a `Command` whose `pre_exec` hooks `ProcessSpec` cannot express.
Re-deriving either would mean re-deriving syscalls the existing code already
gets right, on the same commit that moves the wait.

So the descriptor is created exactly as before and handed to the loop. What
moves is the **wait**, which is the thread this phase deletes; what stays is
every syscall Perry already got right.

### The `dup` contract, and why sends had to move too

dgram keeps a duplicate for `setsockopt`/`getsockname`, because turnloop does
not expose the descriptor it owns (the same gap P1 hit with `setNoDelay` and
the TLS handoff). `dup(2)` shares one open file description, which is what makes
the retained copy useful — an option set through it is the same `setsockopt`,
and `getsockname` answers about the same binding.

It is also what makes it dangerous: `O_NONBLOCK` is a property of that shared
description, and turnloop sets it on adoption. A `send_to` through the retained
copy would therefore have started failing with `EWOULDBLOCK` the moment the
socket buffer filled, where it used to block. **That is why dgram sends moved to
the driver rather than only receives** — not as an optimisation. It is also
closer to Node, whose `send()` is asynchronous and reports through its callback.

`the_retained_duplicate_names_the_same_socket_as_the_adopted_one` pins the half
of this that the whole design rests on: without it, every multicast option
would silently apply to nothing.

## GC decisions

**No JS heap memory reaches the driver.** Reads land in turnloop's pooled
buffers and are copied out inside the dispatch call, on the owning thread, into
the same `Vec<u8>`-carrying queue entries the deleted threads pushed; sends hand
over an owned `Vec<u8>` the caller had already copied out of the JS value. So
there is nothing to root across a collection and no pointer for a moving
collector to invalidate — the property P1 established, unchanged, and the reason
`turnloop_proc` registers no root scanner of its own.

What is new is a rooted JS value with a completion-scoped lifetime.
`socket.send(msg, cb)` used to complete synchronously, so `cb` only had to
survive a microtask. It now completes on a later turn, so `cb` is held in
`dgram_reactor`'s `PendingSend` and visited by that module's **already
registered** `scan_roots_mut` — rooted from submit to completion, released
exactly once at the completion (DESIGN D3/D4). A submission the driver refuses
releases it at the refusal, rather than leaving a root nothing will ever claim.

The JS-side records are otherwise untouched: `dgram_reactor::scan_roots_mut` and
`cp_reactor_scan_roots_mut` still own the socket and ChildProcess values through
`gc_register_mutable_root_scanner`. That is deliberate — moving the *producer*
off a thread must not move the *roots*, or the phase would be two changes at
once. `scripts/gc_runtime_root_holders.py` is green with no new inventory entry:
the new holder's scanner lives in the same file as the holder it scans.

## Behaviours that needed explicit handling

Four things the threads got from their structure that a completion model does
not.

1. **A datagram must not arrive after `'close'`.** The thread path removed the
   registry entry inside `close()`, so `pump` skipped anything already queued.
   A loop entry instead outlives `close()` by however long the driver takes to
   acknowledge, so a datagram received just before the close would have reached
   `pump` afterwards and emitted `'message'` on a socket JS had already seen
   `'close'` for. The entry is marked closing at `close()` and both the
   completion path and `pump` honour it.
2. **A refused send must hand its bytes back.** A socket on the thread fallback
   still has a reactor id, so the first version moved the `Vec` into the loop
   path, found no entry, and reported `EBADF` — dropping the datagram and
   failing *every* send on a thread-backed socket. `send_on_loop` now returns
   the buffer in a `NotOnLoop` variant, and the two refusals are separate
   variants rather than an empty-buffer sentinel, because a zero-length
   datagram is a real datagram (Perry's own close path sends one).
3. **A `connect()`ed dgram socket has no kernel peer.** Node's
   `socket.connect()` is bookkeeping in Perry — `dgram/ops.rs` sets hidden
   fields and never calls `connect(2)` — so routing a connected socket's send
   through the driver's stream write would have failed with `EDESTADDRREQ`. The
   destination `send_destination` already resolved is always passed.
4. **A signal subscription must be named by its entry, not its number.** An
   `off()` immediately followed by an `on()` for the same signal — which is what
   `process.once` does on every delivery — produces two entries whose lifetimes
   overlap. Keying on the signal number let the second overwrite the first, and
   the first's terminal completion then released the *second*: the new listener
   silently stopped receiving, with nothing to see at the point of failure.

5. **A pump has to turn the loop before it drains its queue.** A subsystem's
   pump used to be self-sufficient: a thread had already pushed the bytes, so
   draining the queue was the whole job. With a completion-shaped transport the
   bytes exist only once the loop has been turned, so any caller that drives a
   pump *without* parking — the `await` poll loop in a real program, and the
   `child_process` lifecycle tests, which is where this surfaced — spins
   against a queue nothing can fill. Both pumps now take one nonblocking turn
   first, which costs a thread-local length read and no syscall when this
   thread has adopted no descriptor.

Three of the first four were found by reading the code rather than by a failing
test, and none of those is visible on the arm the acceptance tests exercise.
The fifth was found by the unit suite, and is the one a reviewer should look at
hardest: it is the difference between "a thread pushed this already" and "this
exists when I ask for it", and every later phase inherits it.

## Signals: what moved and what the wake thread costs now

`process.on('SIGINT', …)` installed a `sigaction` whose handler wrote one byte
to a self-pipe, and started a `perry-signal-wake` thread whose entire existence
was to block in `read(2)` on the other end and call `js_notify_main_thread()`.
That thread was started by the **first signal listener of any kind**.

Where turnloop has a portable name, its process-wide dispatcher now fans the
signal out to this agent's loop and the completion lands on the thread that owns
the JS heap, where it bumps the very same `pending` counter the handler bumped.
Everything downstream — `take_pending_process_signals`,
`js_process_signal_drain`, the listener-count re-sync, the exit-code mapping —
is untouched, because the only thing that changed is who produces the wake.

SIGQUIT, SIGABRT, SIGBUS and SIGPIPE have no portable turnloop name and keep
`sigaction`; ABRT and BUS are co-owned by the GC quarantine reporter, so
dropping them was never an option. **The wake thread now starts only if one of
those four is actually subscribed**, so a program that handles SIGINT and
SIGTERM — every CLI with a graceful shutdown — starts none.

The subscription is created **unref'd**, which encodes the ref-neutral
invariant (`has_active_process_signal_listeners` gates on `pending > 0`, not on
`listeners > 0`; `crates/perry/tests/issue_signal_listener_ref_neutral.rs` is
the regression test) in the transport instead of leaving a second counter to
undo it.

## Test evidence

All commands as run.

### Runtime unit tests — real descriptors, on the real driver

```
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-runtime turnloop_proc
```

→ **9 passed**, on Linux x86_64 (epoll). They are loopback tests against the
actual `Loop`, not mocks: a UDP round trip asserting both the payload and the
source endpoint; a receive proven to rearm across three datagrams (turnloop's
UDP receive is single-shot, so "keeps receiving" is *this module's* property,
not the driver's, and three is the smallest count that distinguishes it from
"delivered the first and stopped"); three sends draining the queued count with
their caller tokens echoed back in submission order; an oversized datagram's
`EMSGSIZE` reaching the submitting token; a pipe streaming to EOF; exactly-once
close under a double `close()`; a refused submission for an unknown id; the two
token spaces proven disjoint; and the assumption the dgram design rests on —
that `setsockopt` and `getsockname` through the retained duplicate act on the
same socket the driver is receiving on.

Each pairs its byte assertion with an event-kind assertion, and every fixture
checks `live_handles()`, so a run that adopted nothing cannot pass.

The whole runtime suite, both arms, same host, `RUST_TEST_THREADS=1`:

```
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-runtime
```

| arm | passed | failed |
|---|---|---|
| baseline `c6f185d6e8` | 3965 | 2 |
| **P2** | **3974** | **2** |

The same two in both arms:
`gc::tests::heap_generation::a_free_or_move_outside_every_scope_is_caught_in_debug_builds`
(the `perry-dev` profile inherits `release`, so the `debug_assert!` it asserts
on is compiled out) and `native_stack::tests::stack_top_respects_custom_thread_stack_sizes`.
The nine extra passes are this phase's own tests.

```
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-ffi        → 39 passed
RUST_TEST_THREADS=1 cargo test --locked --profile perry-dev -p perry-ext-net --lib → 36 passed
```

An intermediate state of this branch failed 14 of those tests, and the way they
failed is worth recording: three `child_process` lifecycle tests timed out
(they drive `cp_reactor_pump()` without parking — the fifth behaviour above),
and **eight `event_pump` / `stdlib_pump` tests that have nothing to do with
this change failed behind them**, because `RUST_TEST_THREADS=1` puts every test
in one process on one thread and a lifecycle test that timed out with a live
child and adopted handles poisoned everything after it. Fixing the first three
fixed all eleven. Reading those eight as separate regressions would have cost a
day.

### Thread counts — the point of the phase

`threads_probe.ts` reads `/proc/self/task` while a representative workload is
live: three bound dgram sockets, three signal listeners, and two children with
piped stdout and stderr. Same probe, same host, one compiler apart.

| live workload | baseline `c6f185d6e8` | **P2** | Node 26.5.1 |
|---|---|---|---|
| idle | 1 | 1 | 7 |
| + 3 bound dgram sockets | 4 | **1** | 7 |
| + 3 signal listeners | 5 | **2** | 7 |
| + 2 children, stdout+stderr piped | 11 | **4** | 7 |

Read the deltas rather than the totals. Three dgram sockets cost three threads
and now cost none. Signal listeners cost one process-wide thread and now cost
one — but it is turnloop's own signal dispatcher, shared by every subscribed
signal, where the old one existed per process from the first listener of *any*
signal; and a program subscribing only to the four turnloop cannot carry still
pays the old thread. Two children cost six threads (two readers each plus a
waiter) and now cost two: **the waiters, which this phase did not move.**

### `PERRY_LOOP_STATS` — the subject ran

The P2 gap test, same binary shape, one compiler apart:

```
baseline  [perry-loop] driver=turnloop turns=16 os_waits=8 zero_event_waits=8 \
                       native_ticks=0 turn_errors=0 completions=0
P2        [perry-loop] driver=turnloop turns=63 os_waits=7 zero_event_waits=19 \
                       native_ticks=0 turn_errors=0 completions=103
          [perry-loop] p2 adopted=10 live=0 dgram_sockets=0 signals=0
```

`completions=0 → 103` is the load-bearing number: the baseline turns the loop
(P0 made it the wait primitive) but carries **no I/O on it at all** for this
workload, because every byte arrives on a thread. `adopted=10` is the P2 line's
own counter — a lifetime count, because every live count is zero by the time a
process exits, so a live count would report nothing about a workload that has
finished.

**The two arms' stdout is byte-identical** (`diff` over the full run), which is
what says the 103 completions replaced the threads rather than joined them.

The thread probe tells the same story from the other end: on the baseline it
turns the loop 6 times and dispatches **0** completions while holding eleven
threads; on P2 it turns 44 times, dispatches 16 completions, and holds four.

### Gap suite

`test-files/test_gap_turnloop_p2_process.ts` is new: a UDP round trip with
three sends and their callbacks, `rinfo` asserted to name the sender, the
post-bind option setters, an explicit close; a child whose stdout and stderr
both carry bytes and whose exit code is 7; a child writing 256 KiB so the
multishot read has to deliver more than one pipe buffer; and five signals
delivered to `process.on` — the four turnloop carries plus SIGQUIT, which it
does not, so one program covers both transports.

It prints no port, pid, path or errno, because those are host-specific and
asserting them would make the test about the platform rather than the
behaviour.

```
export PATH=/opt/node-v26.5.1-linux-x64/bin:$PATH
export PERRY_SKIP_BUILD=1 PERRY_RUNTIME_DIR=$PWD/target/release
./run_parity_tests.sh --filter test_gap_turnloop_p2
```
→ **Parity Pass 1, Fail 0, Crashed 0**, byte-identical to the oracle.

`PERRY_RUNTIME_DIR` is not optional on that host: `/etc/profile.d/perry.sh`
exports it pointing at a *different* checkout, so a run that does not override
it links someone else's archives.

The same command over the other tests in these areas:

| filter | pass | fail | compile-fail | crash |
|---|---|---|---|---|
| `test_gap_turnloop_p2` | 1 | 0 | 0 | 0 |
| `test_parity_dgram` | 1 | 0 | 0 | 0 |
| `test_gap_9493_child_stdin` (backpressure) | 1 | 0 | 0 | 0 |
| `test_issue_1933` (fork IPC) | 1 | 0 | 0 | 0 |
| `test_gap_9416_stdin` (stdin-only loop liveness) | 1 | 0 | 0 | 0 |

### node-suite — the behavioural corpora

Run against a baseline built from this branch's own pre-migration commit
(`c6f185d6e8`), because a corpus that is partly red at baseline cannot be read
from one arm.

```
export PATH=/opt/node-v26.5.1-linux-x64/bin:$PATH PERRY_SKIP_BUILD=1
PERRY_RUNTIME_DIR=<tree>/target/release ./run_parity_tests.sh --suite node-suite --module <m>
```

| module | fixtures | baseline pass | **P2** pass | rows changed |
|---|---|---|---|---|
| `dgram` | 57 | 41 (73.2 %) | **41 (73.2 %)** | none |
| `child_process` | 53 | 53, 53, 52 † | **53, 53, 52 †** | none |
| `process` | 105 | 105 | **105** | none |

† `sync/sync-options` is flaky at load in *both* arms; see below.

For `dgram` the *per-test* lists are identical: the same 15 output mismatches
and the same single compile failure (`send/blocklist`) in both arms. The
committed floor for this module is 42 of 57
(`test-parity/node_suite_baseline.json`); this branch neither raises nor lowers
the number it started from.

Two findings worth recording, because both would otherwise be misread:

- **`dgram/multicast/reuse-address-cleanup` regressed, and was fixed.** It was
  the *only* row that moved in the first P2 run. The fixture closes two sockets
  sharing a port and rebinds a fresh one — without `reuseAddr` — on the next
  statement. An asynchronous close makes that EADDRINUSE. `close_and_settle`
  drives the close to its terminal completion, and the row is back to pass. It
  is worth the paragraph because it is precisely the class of thing a
  "completions arrive later" migration breaks, and nothing in the unit tests
  could have caught it: the release is only observable from *outside* the
  module.
- **`child_process/sync/sync-options` is flaky under load, in both arms.** The
  first baseline sweep showed 52/53 with that one mismatch while the first P2
  sweep showed 53/53, which would have read as a P2 *improvement*. It is not.
  Run alone it passes 3/3 on the baseline; two further full-module baseline
  sweeps give 53/53 each; and four full-module P2 sweeps give 53, 53, 53 and
  52. Same fixture, same failure, both arms, roughly one sweep in three. It is
  a `spawnSync` test that this phase does not touch. Claiming either the
  improvement or the regression would have been wrong, which is exactly why the
  baseline was re-run before either was written down.

### GC stress with I/O pending

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<s> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 ./gap_p2
```

Clean at seeds 1, 7 and 12345 — exit 0, no SIGSEGV from the quarantine
reporter, and **stdout byte-identical to the unstressed run**. The instruments
are shown to have been armed rather than merely quiet:

| seed | copying minors | objects moved | from-space quarantines | gc diagnostic lines |
|---|---|---|---|---|
| 1 | 80 | — | 40 | 1,969 |
| 7 | 80 | 14,799 | 40 | 1,969 |
| 12345 | 82 | — | 41 | 2,002 |

Seed 7's own verdict line:

```
[gc-schedule] done: seed=7 safepoints=40 scheduled_collections=40 polls_paced=0 \
              copying_minors=40 moved_objects=14799 loop_polls=10
```

A run with zero copying minors quarantines nothing and would have passed
vacuously; 40 `[gc-fromspace-protect] retired_set=` lines say the from-space
really was detached, poisoned and `mprotect`ed, and 14,799 moved objects say
survivors really were copied. The same runs report `completions=49`, so those
collections landed while dgram and child-pipe operations were in flight.

## Remaining tokio and remaining threads in these paths

| site | what still uses a thread or tokio |
|---|---|
| `child_process/reactor.rs` `cp_spawn_waiter` | one thread per child, blocked in `Child::wait()` |
| `child_process/reactor.rs` `cp_spawn_ipc_reader{,_advanced}` | one thread per forked child |
| `child_process/reactor/stdin.rs` `cp_spawn_stdin_drain` | one thread per child, lazily, on a backed-up pipe |
| `pty/reactor.rs` `pty_spawn_reader` + its waiter | two threads per pty |
| `os_process_streams.rs` `ensure_stdin_reader` | one process-wide `process.stdin` reader |
| `os/signal.rs` `ensure_signal_wake_thread` | one process-wide thread, now only for SIGQUIT/ABRT/BUS/PIPE |
| `dgram_reactor.rs` `spawn_recv` | the fallback for an agent with no loop |
| `child_process/reactor/streams.rs` `cp_spawn_reader_thread` | the same fallback |

None of these is tokio: every thread in this phase's scope is a plain
`std::thread`. tokio's remaining presence in `child_process` is nil — the
subprocess path never used it.

## For the integrator

- Full gap suite, both tiers, and `cargo test --workspace`.
- `./run_parity_tests.sh --suite node-suite --module dgram` / `child_process` /
  `process` against a baseline from `c6f185d6e8`. Expect no row to move; treat
  `child_process/sync/sync-options` as a known flake, not a signal.
- A **Windows arm**. Nothing here ran on Windows and three things are only
  exercised there: `Detached::from_socket`/`from_handle` adoption, the
  `windows_fork` child's pipes, and the console-control-event half of the signal
  table (which this change does not touch — the Windows `SetConsoleCtrlHandler`
  path is untouched and still uses its own wake).
- An instruction A/B at cgu=1 with a control probe, on a dgram- and
  child-heavy workload. The tokio arm is `--features perry-stdlib/tokio-wait-driver`,
  which turns the loop off and puts every subsystem back on its threads.
- `scripts/gc_runtime_root_holders.py`, `scripts/check_file_size.sh` and
  `scripts/addr_class_inventory.py` are green on this branch (run directly).
- Three `-D warnings` dead-code errors exist on this branch **at the default
  feature set and also on the base commit** — `registered_extern_handle`,
  `wasm_memory_descriptor_maximum` and an `ic_slow.rs` unused assignment. They
  are not this phase's, and the `warnings` CI job evidently builds a different
  feature set; worth confirming before reading a red `warnings` job as ours.
- The trees are on the build box at `/root/claude-turnloop-p2/perry` (this
  branch) and `/root/claude-turnloop-p2/base` (`c6f185d6e8`), each with its own
  `target/`. Delete both when the A/B is done. Both need `PERRY_RUNTIME_DIR`
  overridden per the note above.

## The turnloop API this phase wants next

In the order that unblocks the most:

1. **`ProcessSpec` with more than three child descriptors**, plus a session
   (`setsid`) option — unblocks moving the whole child spawn, and with it the
   per-child waiter thread. Alternatively `Loop::adopt_process(pid)`, which
   moves the waiter alone.
2. **A way to read a live handle's descriptor** (`Detached::as_fd()`, or
   `Loop::set_option`), which would remove the `dup` this phase relies on and
   with it the shared-`O_NONBLOCK` hazard that forced dgram's sends onto the
   driver. P1 asked for the same thing for `setNoDelay` and the TLS handoff;
   three phases now want it.
3. **Portable `Signal` names for QUIT, ABRT, BUS and PIPE**, which would let the
   `perry-signal-wake` thread and its self-pipe be deleted outright rather than
   made conditional.
