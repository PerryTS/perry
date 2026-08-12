# #7803 — the zod dep-corpus under `PERRY_GC_SCHEDULE_RATE=1`, quarantine OFF

Working notes, written incrementally. Worktree `/Users/amlug/projects/perry/wt-zod`
at `bdfcba4a2` (v0.5.1499), `CARGO_TARGET_DIR=$HOME/cargo-targets/zod`.

## 0. The corpus links again

This was the blocker: `test-files/gc-dep-corpus/main.ts` had not linked all
week (45 undefined symbols, every one naming the `core/index.ts` barrel), filed
as #7964. **#7980 (`fec46413d`, "resolve re-exports through star barrels")
fixed it.** On `bdfcba4a2`:

```
PERRY_RUNTIME_DIR=$HOME/cargo-targets/zod/release PERRY_NO_AUTO_OPTIMIZE=1 \
PERRY_DISABLE_BUILD_CACHE=1 \
  $HOME/cargo-targets/zod/release/perry test-files/gc-dep-corpus/main.ts -o /tmp/zod-w
→ exit 0, "Wrote executable: /tmp/zod-w", 29.9MB, 5m09s
```

No workaround, no regeneration, no re-pin: the corpus is the one in the tree,
against the `zod@4.3.5` that `package-lock.json` pins today.

Plain run (no schedule), the answer everything else is compared against:

```
endpoints=9
parsed=96
registered=9
GET https://registry.example.com/v1/alerts [alerts|read|get|abs] {-} #2
```

## 1. #7803 does not reproduce

The filed condition, verbatim from the issue (quarantine **OFF**):

```
PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1 /tmp/zod-w
```

Three runs, all **exit 0**, stdout byte-identical to the plain run:

| run | forced_collections | copying_minors | moved_objects | loop_polls | wall |
|---|---|---|---|---|---|
| 1 | 6627 | 6627 | 871,656 | 63,936 | — |
| 2 | 6652 | 6652 | 872,806 | 63,936 | 135.9 s |
| 3 | 6909 | 6909 | 876,805 | 63,936 | 145.4 s |

The instrument is live rather than assumed: 6.6k copying minors relocating
~872k objects per run. The issue's failure was at safepoint 3,319 — well inside
this range — so a run that reaches 6,600+ has passed straight through the
window that used to kill it.

### The seeded schedule does NOT replay exactly

The issue asked for this to be confirmed first, and the answer is **no**:
`loop_polls` is stable at 63,936, but `safepoints` / `scheduled_collections` /
`moved_objects` drift ~4% run-to-run at a fixed seed (6627 → 6652 → 6909). So
`(seed, counter)` is not a complete description of the schedule on this
workload; something outside the counter (event-loop-boundary safepoints,
allocation pacing) varies. Worth its own note — a seed is a strong bias, not a
replay. Everything below depends on this: a seed that fails does not fail every
time, and a seed that passes has not been cleared.

## 2. The candidate cause is REFUTED — it was not #7962/#7978

`ROOTVEC-NOTES.md` named `Object.defineProperties` / `Object.defineProperty`
(#7962, #7978) as the candidate, explicitly unconfirmed. Sabotage A/B, fix
committed on `main` and reverted underneath it:

```
git show cf9999855^1:<f>  →  object/groupby.rs, object/object_ops/define_properties.rs, proxy/own_keys.rs
git show 73109804b^:<f>   →  object/object_ops/define_property.rs, descriptor_helpers.rs, reflect_support.rs
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
```

Archives moved (20:44/20:47 → 21:06/21:08), the relinked corpus binary differs
from the unsabotaged one (`cmp` non-identical, so the sabotage really reached
the link), and the sabotaged arm under the filed condition:

| arm | seed 1, rate 1, quarantine OFF |
|---|---|
| `main` (fixed) | exit 0 ×3, answer byte-exact |
| **#7962 + #7978 reverted** | **exit 0 ×2, answer byte-exact** (6532 / 6544 copying minors, ~865k moved) |

So reverting both fixes does **not** bring #7803 back. They are not what closed
it. Tree restored and **rebuilt** afterwards (archives 21:25/21:28), not
`git checkout`-ed and trusted.

The workload is not the variable either: `test-files/gc-dep-corpus/` is
untouched since #7311, and the installed `zod` is `4.3.5`, the same version
`package-lock.json` has pinned throughout. The only delta between v0.5.1458
(where #7803 was observed) and v0.5.1499 is compiler + runtime.

## 3. The class is STILL LIVE on `main` — it moved to another seed

`RATE=1 TIMEOUT=1800 KEEP=1 PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1
scripts/gc_schedule_fuzz.sh /tmp/zod-w 16`

**seed 4 FAILS**: `TypeError: value is not a function`, exit 1, at safepoint
738. Instrument live at the point of death:

```
[gc-schedule] done: seed=4 safepoints=738 scheduled_collections=738
              polls_paced=5088 copying_minors=738 moved_objects=130683 loop_polls=5826
```

`TypeError: value is not a function` is the canonical late-surfacing form of the
#7154 class — the same class #7803 reports, with a different surfacing message
(#7803 saw `Cannot read properties of undefined (reading 'toString')`). Nothing
had been printed yet, so it dies inside module init / `describeAll()` /
`parseLoop(96)` / `parseRegistered()`, all of which run before the first
`console.log`.

### The quarantine still hides it, exactly as #7803 predicted

Same seed 4, quarantine ON at depth 800: **exit 0**, answer byte-exact, and the
instrument is saturated rather than absent —

```
[gc-fromspace-protect] mode=ProtectPages retired_set=#999 blocks=2
    sets_held=800/800 bytes_protected=2095054848 bytes_poisoned=0 blocks_recycled=398
```

6,888 retired sets, 2.09 GB held, and the run reaches 6,888 copying minors
instead of dying at 738. This is #7803's reading (1) confirmed on a second seed:
holding retired from-space pages out of Eden changes *which* addresses get
recycled and the vulnerable window stops lining up. The protected arm is
therefore not evidence of health, and the shipped witness
(`scripts/gc_dep_scale_witness.sh`, quarantine ON) cannot catch this class of
window on this workload.

### It is stochastic, not seed-determined

Seed 4 re-run: **passes**, 2/2 — the same seed that failed in the sweep.
Consistent with §1's non-determinism: the seed biases the schedule, it does not
fix it. So the correct statement is "the corpus fails intermittently under a
rate-1 unprotected schedule", and any single passing run (including #7803's own
seed 1, now 6/6 clean) is weak evidence.

> Aside, found while trying to hold that A/B still: **`PERRY_GC_DIAG=0` turns
> diagnostics ON.** `telemetry.rs:11` reads it with
> `std::env::var_os("PERRY_GC_DIAG").is_some()`, so any value — `0`, `off`,
> `false` — enables it. My two "DIAG=0" runs above emitted 13,138
> `[gc-copy-minor]` lines apiece; the two arms I thought I was comparing were
> identical. This is exactly the shape CLAUDE.md's knob kill-policy is about, and
> it is a live footgun for anyone reading a `=0` in a repro command as "off".
> (`PERRY_GC_PROTECT_FROMSPACE` does **not** have this bug —
> `arena/quarantine.rs:131` matches `1`/`on`/`true`/`poison` and falls through to
> `Off`, so the issue's `=0` really is off.)

## 4. Full sweep: 3 of 16 seeds fail, in at least TWO distinct ways

`RATE=1 TIMEOUT=1800 PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1
scripts/gc_schedule_fuzz.sh /tmp/zod-w 16` → 13 pass, 3 fail:

| seed | failure | safepoint | class |
|---|---|---|---|
| 4 | `TypeError: value is not a function` | 738 | #7154 late-surfacing |
| 15 | **exit 134**, `[gc-pin-latch] FATAL` | 1870 | pinned-young relocation |
| 16 | `TypeError: Cannot read properties of undefined (reading 'issues')` | 626 | #7154 late-surfacing |

Seed 16's message is the same *shape* as #7803's own
(`Cannot read properties of undefined (reading 'toString')`) — a property read
on a value that should have been an object. So #7803's symptom is alive on the
corpus; only its seed-1 window closed.

### Seed 15 is a SEPARATE bug, and the runtime names it itself

```
[gc-pin-latch] FATAL: copying minor is about to relocate a PINNED young object
  on a preflight-skipped cycle. header=0x2db2f681350 obj_type=8 size=731 flags=0x37
The young-pin latch (gc/pin.rs) is incomplete: some site sets GC_FLAG_PINNED
without going through gc::pin_object. Find it with `python3 scripts/gc_pin_sites.py`
and route it through pin_object (#7645).
```

This is `copying.rs:691`'s deliberate latch, added by #7645, doing exactly what
it was built for — so this is a *detected* fault, not a silent one. Decoded:

* `obj_type=8` = `GC_TYPE_MAP`. The corpus holds its registry in Maps
  (`SCHEMAS`, `CALLBACKS` in `shared.ts`), read via `SCHEMAS.forEach` /
  `.get(...)` in `parseRegistered()`.
* `flags=0x37` = `MARKED | ARENA | PINNED | INTERNED | TENURED`. Note
  **`TENURED` is set on an object the copying minor is treating as young** —
  that combination is itself worth explaining.

**The FATAL message's own remediation does not apply here.** It tells you to run
`scripts/gc_pin_sites.py`; on this tree that reports

```
gc_pin_sites: OK — every pin originates in gc::pin_object
              (2 allowlisted exception(s), 56 GC_FLAG_PINNED tokens scanned).
```

So the stated hypothesis ("some site sets GC_FLAG_PINNED without going through
gc::pin_object") is *not* the explanation for this instance. Either one of the
two allowlisted exceptions is responsible, or the pin is legitimate and the
defect is in the preflight-skip decision (`preflight_walks_decided`) rather than
in pin bookkeeping. #7645 is **closed**, so this needs a new issue rather than a
reopen — and the FATAL text should stop asserting a cause its own tool refutes.

## 5. Every failure here is INTERMITTENT — quote the rate, not the seed

Re-running the three failing seeds on the same binary:

| seed | in the sweep | on re-run |
|---|---|---|
| 4 | FAIL | pass, 2/2 |
| 15 | FAIL (abort) | pass, 3/3 |
| 1 (#7803's) | pass | pass, 6/6 |

So a per-seed verdict is not reproducible on this workload, and neither
"#7803's seed passes" nor "seed 4 fails" is a durable statement. The durable
one is the **rate**: at rate 1 with the quarantine off, **3 of 16 runs failed
(~19%)**. That is the number to A/B a candidate fix against, and 16 runs is a
thin sample for it — a fix claiming to close this needs a sweep wide enough that
19%→0% is distinguishable from luck (at ~19%, a 16-run clean sweep is only
~3% likely by chance; a 40-run clean sweep is ~0.02%).

## 6. What this means for #7803

**Do not close it.** The specific reproducer in the issue (seed 1) no longer
fails, but:

* the *class* it reports is still live on the corpus, at ~19% of runs;
* one of the three observed failures (seed 16) carries the same message shape as
  the one filed;
* the seed-1 result is not attributable to any fix — §2 refutes the only
  candidate on record, and the seed is not reproducible anyway (§5), so "seed 1
  passes now" is a coin landing the other way, not a repair.

The honest update is: the issue's *reproducer* is stale, its *subject* is not.
Retitle it around the rate, or close it in favour of a fresh issue that quotes
§4's table.

## 7. Left open

* **The rooting defect itself is not localized.** The instrument that would
  localize it (`PERRY_GC_PROTECT_FROMSPACE=1`, depth 800) *suppresses* the
  failure on this workload (§3), which is #7803's own hypothesis (1) confirmed
  on a second seed. A 24-seed sweep over the PROTECTED arm — the issue's
  suggested way out — was **still running when this note was written** (7 of 24
  seeds, no protected-arm fault yet). That is a partial result, not a negative
  one: re-read `/tmp/zod-fuzz-prot.log` before quoting it, and note that at
  ~4 min/run a protected sweep wide enough to matter is a multi-hour job.
* **Phase localization is inconclusive.** A marker-instrumented copy of the
  corpus (`/tmp/zod-probe`, `console.error` between and inside `describeAll` /
  `parseLoop` / `parseRegistered`) survived every seed it was given — the
  markers themselves allocate and perturb the schedule, which is the recurring
  problem with this bug.
* **The pin-latch abort (§4) has no owner.** Distinct from the rooting class,
  self-detecting, and its printed remediation is refuted by its own tool.
* No fix is proposed here, so nothing is landed beyond this note. There is
  deliberately no new gate: a gate for a 19%-of-runs intermittent failure would
  be flaky in CI, and CLAUDE.md's four-ways-a-gate-cannot-fail applies in
  reverse — a gate that goes red 19% of the time on a *healthy* tree teaches
  people to ignore it.
