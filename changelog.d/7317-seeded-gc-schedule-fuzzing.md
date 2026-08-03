### Added

**Seeded GC-schedule fuzzing: `PERRY_GC_SCHEDULE_SEED` (#7154 tooling).**

A rooting bug is a value live but not rooted across a collection point.
Whether it is *caught* is decided by the GC schedule, not by the bug — so
re-running one binary sixty times re-runs one schedule sixty times and explores
almost nothing. Two settings existed: normal pacing (collections tens of
megabytes apart) and `PERRY_GC_ZEAL=1` (collect at every safepoint). This is the
middle, and it hands back a reproducer.

`PERRY_GC_SCHEDULE_SEED=<u64>` makes *"should this safepoint collect?"* a
deterministic pseudo-random function of the seed and a per-thread safepoint
ordinal, at a density set by `PERRY_GC_SCHEDULE_RATE` (default `0.05`).
`scripts/gc_schedule_fuzz.sh <binary> [seed-count]` sweeps seeds and prints a
reproduce command for each failure.

**It converts Socket Firewall's registry ghost into a deterministic
reproducer.** `sfw-registry --help` (#7291's tree, `PERRY_FORCE_WELL_KNOWN=iovalkey`,
compiled *and* run with `PERRY_GC_MOVING_LOOP_POLLS=1`, `--debug-symbols`) fails
about **1 run in 60** in the plain-polls configuration, and that failure has cost
days precisely because it cannot be summoned. Measured on the same binary,
macOS arm64, four runs in parallel:

| arm | failures | time per run |
|---|---|---|
| control, no seed | **0 / 16** | 55 s (all completed) |
| `PERRY_GC_SCHEDULE_SEED=1..12`, `RATE=0.05` | **6 / 12 failed in ≤ 2 s** | 6 remaining censored at 120 s |

The six failing seeds split into two signatures, each stable across the seeds
that produce it:

```
seeds 1, 7, 12 → TypeError: value is not a function
                   at node_modules/zod/src/v4/classic/schemas.ts:1318
seeds 8, 9, 11 → TypeError: Cannot convert undefined or null to object
                   at node_modules/node-machine-id/dist/index.js:1
```

The first is the #7154-class signature the registry investigation has been
chasing. The second is the `node-machine-id` failure that makes `PERRY_GC_ZEAL=1`
unusable on this workload — at 5% density it is reachable *without* also losing
the rest of the program, so it can now be studied rather than routed around.

Seed 1 was re-run five times and failed **5/5** at the identical site in ≤ 1 s.
The control's 0/16 is consistent with the known ~1.7% rate (zero failures in 16
runs bounds it at ~19%, which is why re-running was never going to settle
anything); the point is the contrast with 6/12 in two seconds.

**Cost.** A seeded run at rate 0.05 is roughly 5–10× slower than the unseeded
one on this workload, which is why half the sweep is censored rather than passed:
a seed that has not failed in 120 s has not finished either. Failing seeds cost
1–2 s, so a sweep's wall clock is dominated entirely by the seeds that do *not*
find anything — turn the rate down, or the timeout up, depending on which you are
buying.

**What the knobs gate, precisely** — the GC-knob policy in `CLAUDE.md` is
binding, and this repo has repeatedly paid for knobs that gated something other
than their name (`PERRY_GC_FORCE_EVACUATE` inert for every `gc()`-driven test,
#6942/#6946; the matrix's `--pressure` knob disabling the path it measured,
#7024). `PERRY_GC_SCHEDULE_SEED` does exactly three things:

1. `js_gc_loop_safepoint` stops requiring `GC_SAFEPOINT_PENDING` before it
   descends into `gc_safepoint_moving_minor` — the same bypass zeal performs, and
   needed for the same reason: a schedule cannot select a safepoint the gate
   already returned from.
2. Inside `gc_safepoint_moving_minor`, **past the entry guards**, a per-thread
   counter advances once per handled safepoint; when `gc_budgeted_due_trigger()`
   reports nothing due, a minor runs anyway iff
   `splitmix64(splitmix64(seed) ^ counter) < threshold`.
3. `gc_force_evacuate_enabled()` becomes true, so a scheduled minor MOVES
   survivors. Without this the mode would promise relocation stress and deliver
   sweep pressure.

It does not bypass the entry guards (and a blocked safepoint deliberately does
*not* tick the counter, so the ordinal sequence tracks the program's safepoints
rather than its allocation state); it does not override `PERRY_GEN_GC_EVACUATE=0`;
it cannot emit loop polls codegen never produced (compile-time
`PERRY_GC_MOVING_LOOP_POLLS=1`, as for zeal); and it never *suppresses* a
pressure-driven collection — the rate is additional density, never less.
A value that does not parse as a `u64` reads as OFF, not as seed 0.

**Determinism, scoped honestly.** The decision reads no wall clock, no address
and no thread identity, so a **single-threaded** program replays a seed exactly.
The counter is thread-local, so a `perry/thread` program gets a deterministic
schedule *per thread given that thread's own safepoint sequence* — but nothing
makes the OS schedule that sequence identically twice, and a global counter would
be strictly worse (it would make even one thread's schedule depend on
interleaving). Multi-threaded reproducers are only as reproducible as their
threading; say which you measured.

**The seed is never lost.** It is printed at startup, at exit
(`[gc-schedule] done: seed=… safepoints=… scheduled_collections=…`, from the
process-exit teardown funnel — perry's exits call `_exit`, so `atexit` alone
would miss them), on panic, and from an async-signal-safe handler for
SIGSEGV/SIGBUS/SIGABRT/SIGILL/SIGTRAP. That handler **chains** rather than
clobbers, and the from-space quarantine re-layers it after installing its own, so
`PERRY_GC_SCHEDULE_SEED=… PERRY_GC_PROTECT_FROMSPACE=1` reports both the seed and
the precise fault site. A fuzzer that finds a bug and loses the reproducer is
worthless.

**Default-off, and proven inert rather than assumed inert.** With no seed set,
`PERRY_GC_DIAG=1` collector traces are byte-identical to the parent commit's
across five configurations on two fixtures — 367-line traces under plain polls,
4941 under zeal, 6151 under zeal + from-space protection, and the no-polls and
forced-evacuation arms besides. Unit coverage in
`gc/tests/schedule.rs` asserts both directions of both knobs (11 tests: parse,
threshold endpoints, 100k-ordinal determinism, adjacent-seed divergence, realised
density, collect/decline/blocked at a real safepoint, and the evacuation
implication with its `PERRY_GEN_GC_EVACUATE=0` precedence arm).
`scripts/gc_instrument_smoke.sh` gains three integrated arms that gate the three
claims end to end: the seeded schedule must retire strictly more from-space
page-sets than pressure alone and strictly fewer than zeal (it is a *middle*
setting, not a second name for one of the endpoints), and the same seed twice
must retire exactly the same number (it is a *reproducer*).

`cargo test -p perry-runtime` on this branch: **1670 passed, 0 failed**
(`--test-threads=1`, two consecutive runs). The branch parent measured on the
same machine in the same conditions: 1658 passed, 1 failed
(`pty::unix_impl::tests::js_pty_spawn_shell_data_and_exit`, a 15 s pty wait that
times out under load). The default parallel mode is flaky on both — three
`object::` failures on the branch, a different four on the parent, none
overlapping — which is a pre-existing test-isolation problem, not this change.

No collector policy changed. Every scheduled collection runs at a point the
collector already treats as a precise-root safepoint; only how often changes.
