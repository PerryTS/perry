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

## 1. #7803's reproducer does not reproduce (but read §3-§5 before quoting this)

The filed condition, verbatim from the issue (quarantine **OFF**):

```
PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1 /tmp/zod-w
```

Three dedicated runs, all **exit 0**, stdout byte-identical to the plain run
(a fourth, seed 1 inside the section-4 sweep, also passed — 4/4 in total):

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
seed 1, now 4/4 clean) is weak evidence.

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
| 1 (#7803's) | pass | pass, 4/4 |

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
  suggested way out — reached **11 of 24 seeds with no protected-arm fault**
  before this note was closed out; it was still running, so treat that as a
  partial result, not a negative one, and re-read `/tmp/zod-fuzz-prot.log`
  before quoting it. At ~4 min/run a protected sweep wide enough to matter is a
  multi-hour job, and §5's arithmetic applies to it too: 11 clean protected runs
  do not clear a 19% failure rate.
* **Phase localization is inconclusive.** A marker-instrumented copy of the
  corpus (`/tmp/zod-probe`, `console.error` between and inside `describeAll` /
  `parseLoop` / `parseRegistered`) passed **12 of 12** seeds, each reaching
  `PHASE: parseRegistered done`. The markers themselves allocate and perturb the
  schedule, so this is the recurring problem with this bug rather than evidence
  about which phase is at fault — a probe dense enough to localize the failure
  is dense enough to prevent it. (Seed 11's marker log reads empty only because
  a disk-pressure cleanup of mine deleted its stderr while that run was still in
  flight; its exit status was 0 like the rest. Recorded rather than quietly
  dropped: housekeeping aimed at a finished experiment hit a live one.)
* **The pin-latch abort (§4)** is filed as **#7990**. Distinct from the rooting
  class, self-detecting, and its printed remediation is refuted by its own tool.
* No fix is proposed here, so nothing is landed beyond this note. There is
  deliberately no new gate: a gate for a 19%-of-runs intermittent failure would
  be flaky in CI, and CLAUDE.md's four-ways-a-gate-cannot-fail applies in
  reverse — a gate that goes red 19% of the time on a *healthy* tree teaches
  people to ignore it.

## 8. Rebuilding this from scratch

The worktree and its `CARGO_TARGET_DIR` were both deleted by whatever sweeps
`/Users/amlug/projects/perry/wt-*` on this box, *while the protected sweep was
still running* — so budget a full rebuild rather than expecting the artifacts to
be there. Everything needed is on the branch; the numbers above came from:

```bash
git worktree add <wt> origin/main
export CARGO_TARGET_DIR=$HOME/cargo-targets/zod        # ~2.5 GB
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static   # ~14 min
npm ci --ignore-scripts --no-audit --no-fund           # or symlink node_modules; the lock pins zod 4.3.5

export PERRY_RUNTIME_DIR=$CARGO_TARGET_DIR/release PERRY_NO_AUTO_OPTIMIZE=1 PERRY_DISABLE_BUILD_CACHE=1
$CARGO_TARGET_DIR/release/perry test-files/gc-dep-corpus/main.ts -o /tmp/zod-w   # ~5 min

# the filed condition (§1)
PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1 /tmp/zod-w

# what actually finds it (§4) — ~145 s/run, budget an hour
RATE=1 TIMEOUT=1800 KEEP=1 PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1 \
  ./scripts/gc_schedule_fuzz.sh /tmp/zod-w 16
```

`PERRY_NO_AUTO_OPTIMIZE=1` is not optional: without it the auto-optimizer
relinks the runtime without `diagnostics`, which removes the very
`[gc-fromspace-protect]` evidence §3 depends on.

---

# Session 2 (2026-08-13, `wt-7803` @ `410dadd45`, v0.5.150x)

Picks up §7 "left open". Worktree `/Users/amlug/projects/perry/wt-7803`, branch
`fix/7803-zod-gc-rooting`, in-tree `target/` (not a separate `CARGO_TARGET_DIR`
this time). Everything below was measured on a **contended box** — three other
worktrees (`wt-1849`, `wt-5497`, `wt-7170`) were building throughout, and the
corpus compile ran at 22% CPU — so wall times here are not comparable with
§8's, and are quoted only to budget a repeat.

## 9. The corpus/lowering matrix has an empty cell, and it is the cell #7803 lives in

`gc-root-dominance.yml` emits three corpora, not four:

|                        | shadow (`PERRY_RS4GC=0`)      | native (statepoints — **what ships**) |
|------------------------|-------------------------------|---------------------------------------|
| curated (~124 files)   | gated (dominance, allocas, `--max-stale 39`) | gated (`--statepoints --max-stale 0`) |
| dependency-scale (zod) | gated (dominance, allocas, `--max-stale 118`) | **never emitted** |

Two separate corrections landed in this file and neither reached the other's
cell:

* **#7280** — the curated corpus is the wrong POPULATION. "25 curated files
  pass while 20 lines of stock zod fail." That added `ir-corpus-dep`.
* **#7452** — the shadow lowering is the wrong LOWERING. Statepoints became the
  default in #7370, so a `PERRY_RS4GC=0` corpus contains zero of the root form
  that ships; the curated corpus "was still emitting 81 modules with 0 bind
  call sites". That added `ir-corpus-native`, curated only.

The intersection — the zod corpus compiled the way the failing binary is
compiled — has never been generated, so its stale/unrooted population is
unmeasured. That is not a small residual either: the curated corpus's own
unfiltered native census reads **1123 unrooted + 321 stale** (the diagnostic
step in the same job), against a gated arm of 21.

`scratchpad/zod/dep_native_corpus.sh` emits it (compile `PERRY_RS4GC=1`, then
the production `STATEPOINT_REWRITE_PASSES` rewrite through `opt`, single-sourced
out of `crates/perry-codegen/src/inprocess.rs` exactly as the curated script
does, plus the same generation-time subject-liveness assertion so an empty
corpus cannot read as a clean one).

**Status: script written, measurement not yet taken.** Do not quote a number
here until it has run.

### 9a. Measured: the empty cell reads 66, where its sibling is gated at 0

`scripts/…/dep_native_corpus.sh` → 81 modules, **52,198 statepoints, 39,073
non-empty live bundles**, 0 rewrite failures (the curated native corpus, for
scale, is 30,033 / 17,478). Then the same mode the curated arm gates on:

```
python3 scripts/gc_root_dominance_check.py ir-corpus-dep-native \
  --statepoints --moving-only \
  --min-files 60 --min-funcs 1200 \
  --min-statepoints 15000 --min-live-bundles 8000 --min-relocates 20000
```

```
=== statepoint hazards: 66  (unrooted: 66, stale: 0)
      28  unrooted/global      19  unrooted/rootread
      18  unrooted/alloc        1  unrooted/capture
      24  sink=js_new_function_construct
      17  sink=js_closure_call1
      16  sink=js_closure_call_apply_with_spread
       6  sink=js_closure_call2
       1  sink=js_array_concat   1  sink=js_rel_ge
       1  sink=js_get_string_pointer_unified
  (277 more suppressed by the #7210 IMMOVABLE_SOURCES box exemption)
```

**The curated corpus in the identical mode is gated at ZERO.** #7725 deleted its
`--max-unrooted` budget precisely because "`--max-unrooted` already defaults to
0, and a budget nobody re-measures is exactly the silently-absorbing kind", and
`gc-root-dominance-statepoints` is green on `main` as of `81a88de40` (verified
2026-08-13). So this is not "the instrument is noisy": it is calibrated to zero
on the curated population and reads 66 on the dependency one.

`unrooted` is the serious class — the checker's own definition is "no
`ptr addrspace(1)` value in the register's cast chain is in the window
statepoint's live bundle. The OBJECT is unprotected: nothing marks it, nothing
rewrites it." That is #7207's shape, and it is the one that produces #7803's
two observed messages:

* 39 of the 66 sink into `js_closure_call1` / `js_closure_call2` /
  `js_closure_call_apply_with_spread` → **`TypeError: value is not a function`**
  (seed 4, both sessions);
* 24 sink into `js_new_function_construct` → a receiver that is not the object
  it was, which is what `Cannot read properties of undefined (reading '…')`
  looks like downstream (seeds 1/16, and the filed `'toString'`).

Zero `stale`, so `root_reload.rs` is doing its job on this corpus; the residual
is the *unrooted* class, which a reload cannot fix — those need a root.

**This does not yet prove any of the 66 is the one that kills the run.** It
says the shipping lowering of this workload carries 66 hazards of exactly the
right shape that no gate has ever looked at. Itemisation and cross-referencing
against a captured backtrace is the next step, not a conclusion to skip to.

## 10. The rate at HEAD: 7 of 16, not 3 of 16

Same command as §4, same corpus, `zod@4.3.5` unchanged, on `410dadd45`:

```
OUTDIR=… RATE=1 TIMEOUT=2400 KEEP=1 PERRY_GC_PROTECT_FROMSPACE=0 PERRY_GC_DIAG=1 \
  ./scripts/gc_schedule_fuzz.sh /tmp/zod-head 16
```

| seed | verdict | safepoints | moved |
|---|---|---|---|
| 1 | **FAIL** `…undefined (reading 'issues')` | 1930 | 271,716 |
| 4 | **FAIL** `value is not a function` | 2602 | 353,678 |
| 10 | **FAIL** `…(reading 'issues')` | 2592 | 352,878 |
| 11 | **FAIL** `value is not a function` | 1581 | 230,109 |
| 12 | **FAIL** `…(reading 'issues')` | 2520 | 345,143 |
| 14 | **FAIL** `…(reading 'issues')` | 1234 | 187,843 |
| 15 | **FAIL** `…(reading 'issues')` | 1641 | 237,617 |
| 2,3,5,6,7,8,9,13,16 | pass | 6804–6931 | ~862k |

**7/16 (44%)** against §4's 3/16 (19%). Fisher exact on the two sweeps is
p≈0.06 — suggestive, not established, and the honest reading is that ONE of
these is true and I have not separated them:

* the class got worse between v0.5.1499 and `410dadd45` (20+ commits, several
  GC-adjacent: #8014, #8023, #8024, #8026), or
* 16 runs is simply too thin to distinguish 19% from 44%.

Deciding it needs the v0.5.1499 binary rebuilt and swept on the same box in the
same session, which is the correct A/B and was not done here. Do NOT quote
"the rate doubled" from this table alone.

Two things it DOES establish, both load-bearing:

* the subject is live on `main` today, so a fix has something to close;
* a failing run dies at safepoint 1234–2602 of the ~6850 a passing run
  completes, i.e. **early** — inside module init / `describeAll()` /
  `parseLoop(96)`, before the first `console.log`. §7's phase probe could not
  localize it because the markers perturbed the schedule; the safepoint counts
  say it without needing a probe.

Note the passing runs are extremely uniform (6804–6931 safepoints, ~862k moved,
`loop_polls` a constant 63,936). The §1 "4% drift" is the same phenomenon seen
at a smaller sample: the schedule is stable to about ±1%, and what varies is
whether the run survives to finish it.

## 11. The debugger is not a usable instrument here — so the runtime got one

The plan was: break on the two throw helpers, read the native stack, name the
compiled JS function that read the lost value. The mechanics all work —

* `js_throw_type_error_property_access` and `js_throw_type_error_not_a_function`
  are `#[no_mangle]` globals and each resolves to exactly ONE location;
* a healthy unscheduled run hits NEITHER (verified), so any hit is the failure
  and nothing else, no filtering needed;
* `--debug-symbols` keeps 1726 `_perry_fn_*` symbols in the corpus binary, so
  the frames have names.

**But the failure does not reproduce under `lldb`.** 4 seeds, all of which fail
natively at 44%, all passed to completion under the debugger (`bt` reported
"requires a process which is currently stopped"). 4 samples is 0.56⁴ ≈ 10% by
chance, so this is *suspicion, not proof* — I stopped rather than spend an hour
proving it, because the fix is the same either way. Disabling lldb's default
ASLR-off (`settings set target.disable-aslr false`) did not bring it back, so
address randomisation is not the discriminator.

> Recorded as a mistake rather than quietly fixed: the sweep piped logs through
> `grep -vE '^\[gc-'`, which also removed the `[gc-schedule] done:` summary —
> the line that proves the run collected anything at all. Those four "passes"
> therefore carry no liveness evidence of their own. (An earlier run of the same
> harness, before the filter, did print `safepoints=6349 copying_minors=6349
> moved_objects=847052`, so the env does reach the target under lldb.) A sweep
> whose logs cannot show its subject ran is the vacuous-green shape, and it took
> a second look to notice.

So the instrument moved into the runtime, where it observes the run that
actually fails: **`PERRY_UNCAUGHT_BACKTRACE=1`** (`exception.rs`) emits a
symbolicated native backtrace on the uncaught-throw path, reusing the
`libc::backtrace` + `backtrace_symbols_fd` pair `arena::quarantine` already
uses. Off by default, parsed BY VALUE (`1`/`on`/`true`) — the `PERRY_GC_DIAG=0`
footgun in §3 is one release old and does not get repeated. It fires at most
once per process, on a path already headed for `exit(1)`.

## 12. What the 66 are: two shapes, 37 of them in the functions this workload calls constantly

Grouped by fingerprint (`scratchpad/zod/dep-native-verbose.txt` has all 66):

| n | module | shape |
|---|---|---|
| 21 | `v4/classic/schemas.ts` | `unrooted:global -> js_closure_alloc` |
| 16 | `v4/classic/schemas.ts` | `unrooted:alloc -> js_array_like_to_array` |
| 10 | `v4/core/errors.ts` | `unrooted:rootread -> js_ctor_return_override` / `js_closure_get_capture_bits` |
| 7 | `v4/locales/he.ts` | `unrooted:rootread -> js_object_get_field_by_name_f64` |
| 5 | `v4/locales/lt.ts` | `unrooted:global -> js_object_get_field_by_name_f64` |
| 2+1+1+1+1 | `core/schemas.ts`, `core/parse.ts`, `core/util.ts`, `core/doc.ts`, `classic/schemas.ts` | tail |

**The 12 locale hits are almost certainly not this bug.** They are inside
`he`/`lt` message-map closures; the corpus never selects a non-`en` locale, so
those bodies never run and a hazard in an uncalled function cannot fire. Worth
fixing, not worth chasing here. That leaves ~54, and 37 of them are two shapes
in one file.

### Shape A — 21× `unrooted:global`, e.g. `strictObject` / `looseObject`

```llvm
%r25 = load double, ptr @perry_global_…_classic_schemas_ts__39   ; module-level var
;   across safepoint: js_closure_alloc, js_closure_call1,        ; ← user code runs
;                     js_closure_set_capture_bits, js_object_alloc
call @llvm.experimental.gc.statepoint.p0(… @js_new_function_construct, %r25-derived, …)
```

A module-level variable holding a constructor is loaded into a register; a
closure is allocated and **called** (`js_closure_call1` — arbitrary user code,
so an evacuating minor is entirely plausible); the pre-move register is then
handed to `js_new_function_construct`. `@perry_global_*` IS a registered root,
so the object survives *at a new address* — property (2) without property (3),
#7240's shape exactly. Constructing from a recycled address yields an object
whose fields read `undefined`, which is what `Cannot read properties of
undefined (reading '…')` looks like one frame later.

**This population is knowingly unhandled, and `root_reload.rs` says so:**

> `is_string_handle_global`: "Narrow on purpose: `@perry_global_*` is a
> module-level variable the PROGRAM assigns, so re-reading it could observe a
> later assignment instead of the value the call was given — **that population
> needs rooting, not reloading, and is deliberately not matched here.**"

That judgement is right — a reload is unsound here — and the rooting it defers
to was never done. What is new is the *count on a real library*: the reason to
prioritise it could not be seen, because the corpus that exhibits it was never
emitted under the lowering that ships.

### Shape B — 16× `unrooted:alloc`, closures 146/147/…

```llvm
%r123 = <allocation result>
;   across safepoint: js_array_like_to_array                     ; allocates
call @llvm…statepoint(… @js_closure_call_apply_with_spread, … %r123 …)
```

A fresh object held in a bare register across the array-like→array conversion
of a spread/`apply` call, then passed to the call. Nothing roots it at all
(#7207's shape, the one `--unrooted-allocas` was built for). Unlike Shape A
there is no soundness objection to fixing it — the value has no other home, so
a temp root is simply the missing code.

### Why this is a hypothesis and not yet a cause

Every hazard here is a *possibility* of a stale/lost value, and the checker is
one-sided by design. Three things would settle it, in increasing cost:

1. a `PERRY_UNCAUGHT_BACKTRACE` stack from a failing run that names one of
   these functions (in flight);
2. fixing Shape A + Shape B and re-sweeping: 7/16 must go to 0/40 for the fix
   to be distinguishable from luck at this rate;
3. sabotage: re-introduce the hazard and show the rate returns.

Note the two shapes have the same fix and it is NOT a reload: root the value
(the `rooting/temp_root.rs` pool already exists and is the mechanism `#7719`
used for the 30 `lower_call/builtin.rs` ctor arms).

## 13. `--debug-symbols` SUPPRESSES the failure — the symbolized build is a different program

This is the finding that explains §11's dead end, and it was found by a control
rather than by reasoning.

Three binaries, same compiler (`410dadd45`), same corpus, same `zod@4.3.5`,
same runtime archives, swept identically
(`RATE=1 PERRY_GC_PROTECT_FROMSPACE=0`, seeds 1..n):

| binary | built with | result |
|---|---|---|
| `/tmp/zod-head` | plain, pre-patch runtime | **7/16 FAIL** |
| `/tmp/zod-bt` | `--debug-symbols`, patched runtime | 0/10 fail (seeds 1–8, 14, 15) |
| `/tmp/zod-plain2` | plain, **patched runtime** | **FAIL on seed 1 and seed 2** |

The third row is the control that makes the second interpretable. Adding the
`PERRY_UNCAUGHT_BACKTRACE` hook to `exception.rs` does NOT suppress the bug —
the plain build carrying that exact runtime still dies at seeds 1 and 2. The
variable that suppresses is **`--debug-symbols`**: at the base rate of 44%, ten
consecutive passes is p ≈ 0.56¹⁰ ≈ 0.3%.

So the debugger was never the problem in §11. I had changed two things at once
(symbols AND lldb) and blamed the wrong one: `/tmp/zod-dbg` was a
`--debug-symbols` build, so those four lldb "passes" were passes of a program
that does not have the bug. Recorded rather than quietly corrected, because the
mistake is instructive — **the instrument that makes a bug observable is also a
change to the program, and it needs its own control arm.**

Why `-g` should move an intermittent GC bug is not established here. It is not
"debug info is inert": `PERRY_DEBUG_SYMBOLS` feeds the object-cache key, the
clang invocation (`-g`), and the final `strip`, and every consumer reads it with
`is_some()`, so there is no spelling that separates them. Two candidates, both
unproven: DWARF changes LLVM's inlining/scheduling enough to move allocation
sites; or the larger image shifts the layout the failure's timing depends on.

### The fix for the instrument: `PERRY_KEEP_SYMBOLS`

Skips ONLY the final `strip` (`post_link.rs`), leaving `-g` off and codegen
byte-identical to the build that reproduces. That is what makes a backtrace
from it evidence about the same program rather than about its symbolized twin.

The stripped instrument already proves itself — `PERRY_UNCAUGHT_BACKTRACE=1` on
`/tmp/zod-plain2` seed 1 emits 11 frames at the fatal throw, 7 of them the
JS/runtime chain, just without names:

```
--- native backtrace at the uncaught throw ---
0   zod-plain2 + 9300712
1   zod-plain2 + 14326752
…
10  dyld  start + 6992
--- end native backtrace ---
[gc-schedule] done: seed=1 safepoints=1162 … copying_minors=1162 moved_objects=179572
```

## 14. LOCALIZED: the fatal frame, at last

`PERRY_KEEP_SYMBOLS=1` (no `-g`) reproduces — seed 3 of 8 — and symbolicates
itself. The first native backtrace of this failure in either session:

```
0  perry_runtime::exception::emit_uncaught_backtrace
1  js_throw + 992
2  js_timer_has_pending + 0                       ← nearest global; really the
                                                    stripped throw helper
3  perry_closure_…zod_src_v4_core_parse_ts__9     + 4132
4  perry_closure_…zod_src_v4_classic_schemas_ts__108 + 76
5  js_native_call_value + 3136
6  js_native_call_method + 24208
7  js_typed_feedback_native_call_method_by_id + 112
8  perry_fn_…gc_dep_corpus_main_ts__parseLoop$spec_i32 + 2272
9  main + 456
```

seed 3: `safepoints=2177 copying_minors=2177 moved_objects=303880`.

### Reading it

`parseLoop`'s `S.safeParse([...])` → the `safeParse` method
(`classic/schemas.ts` closure 108) → `core/parse.ts` closure 9.

**Closure 9 is `_safeParse`'s inner arrow.** Identified from the IR rather than
guessed: it references `$ZodAsyncError`'s class keys (parse.ts:61) and allocates
`perry_closure_…parse_ts__10`, which is the `(iss) => util.finalizeIssue(…)`
callback on parse.ts:68. Only `_safeParse`'s body has both.

```ts
export const _safeParse = (_Err) => (schema, value, _ctx) => {
  const ctx = _ctx ? { ..._ctx, async: false } : { async: false };
  const result = schema._zod.run({ value, issues: [] }, ctx);   // ← line 60
  if (result instanceof Promise) throw new core.$ZodAsyncError();
  return result.issues.length                                    // ← THROWS HERE
```

So the failing read is `result.issues` with `result` **undefined**:
`schema._zod.run({ value, issues: [] }, ctx)` returned undefined.

That reframes the search. This frame is the VICTIM, not the site — it is
`--moving-only` clean, and nothing in §12's list names closure 9. The loss is
upstream, in the `run` chain, and `run` is built in `core/schemas.ts` — which
holds two of the 66 (closures 138 and 185, both `unrooted:rootread`, one
sinking into `js_closure_call1`).

Note both observed messages are the SAME loss seen one call apart: if the
receiver `schema._zod` is a recycled object, `.run` misses and the call throws
`value is not a function`; if the call happens but its result is lost, the
caller reads `.issues` on undefined. That is why the two symptoms alternate
seed to seed and why chasing them as separate bugs would have been wrong.

### Sequencing note, and a correction to §10

§10 read the low safepoint counts as "dies in module init". The backtrace says
otherwise: it dies inside `parseLoop`, which is the SECOND phase. Nothing had
printed because the corpus prints only after all three phases finish — absence
of output was never evidence about phase. §7's marker probe was answering a
question the stack answers directly.

## 15. A hazard from §12's list is ON the stack of a failing run

The other symptom, `value is not a function`, on the same binary (seed 7):

```
 3  throw_not_callable
 4  closure::dispatch::validate::dispatch_proxy_callee_or_throw
 5  js_closure_call2
 6  js_native_call_value
 7  js_native_call_method
 8  dyn_eval::expr::eval_expr           ← part of zod runs INTERPRETED, not native
 9  dyn_eval::interp::exec_stmt
10  dyn_eval::interp::interp_thunk
11  closure::registry::dispatch_with_arity
12  js_closure_call3
13  perry_closure_…core_schemas_ts__137 + 172
14  perry_closure_…core_schemas_ts__138 + 3904      ★
15  js_native_call_value
16  js_native_call_method
17  js_typed_feedback_native_call_method_by_id
18  perry_closure_…core_schemas_ts__115 + 4060
```

**`core/schemas.ts` closure 138 is one of the 66.** Its entry, quoted from
§12's run, predates any of this dynamic evidence:

```
core_schemas_ts.ll::perry_closure_…core_schemas_ts__138   [unrooted]
  source (rootread): %r801333 = gc.result(%statepoint_token332)
  stale use        : statepoint … @js_closure_call1 …
  across safepoint : js_closure_get_capture_bits, js_object_get_field_by_name_f64,
                     js_object_get_field_ic_miss, js_typed_feedback_object_get_field_by_name_f64
  MOVING           : YES
```

A value read out of a root, held across property-get helpers that can run an
evacuating minor, then used as the callee of `js_closure_call1`. The stack
shows 138 calling 137 which calls a closure through `js_closure_call3`, and the
throw is `not callable` on a callee. Same function, same shape, same sink
family.

**This is corroboration, not proof.** Closure 138 is 1,939 lines of IR and the
frame is `+3904` — being on the stack does not establish that the reported
hazard is the instruction that failed. What it does establish is that the two
independent methods now point at the same function, which neither did before
today.

Worth noting separately: frames 8–11 show part of `zod` executing through
`dyn_eval` (the V8-fallback interpreter) rather than natively. Whether that
path's roots are complete is a question this stack raises and does not answer.

## 16. The quarantine still suppresses — second confirmation, and a false alarm

Retried at depth 800 on `/tmp/zod-ks`, whose UNPROTECTED rate is 3/8:

| seed | unprotected | protected (depth 800) |
|---|---|---|
| 3 | FAIL | **pass**, 6822 safepoints, `sets_held=800/800` |
| 4 | pass | pass, 6834 safepoints, `sets_held=800/800` |

Seed 3 is the discriminating cell: it fails unprotected and passes protected on
the same binary in the same session. §3 found this at v0.5.1499 and it holds at
`410dadd45` on a build with a 4× higher base rate. The instrument saturates
(800/800 sets, ~7 GB held), so this is suppression, not absence of instrument.

> **False alarm, recorded because it nearly went in the other direction.**
> Seeds 5 and 6 exited 134 (`Abort trap: 6`) under the quarantine and my first
> reading was "the protector caught it". It did not: the tail says
> `panicked at … failed printing to stderr: No space left on device (os error
> 28)`. The depth-800 arm holds ~7 GB and `PERRY_GC_DIAG=1` writes tens of MB
> per run; the disk filled and the runs aborted on the write, not on a fault.
> An abort under a fault-detecting instrument is exactly the result you want to
> believe, which is why it needed the tail read before it was quoted.

## 17. Where this leaves #7803, and what to do next

### Established this session

1. The class is live on `main` (`410dadd45`) and easy to hit: 7/16 on
   `/tmp/zod-head`, 8/10 on `/tmp/zod-plain2`, 3/8 on `/tmp/zod-ks`. The rate
   is strongly binary-dependent, so **A/B a fix on ONE binary pair, never
   across builds.**
2. The corpus × lowering matrix had an empty cell — the dependency corpus under
   the shipping (statepoint) lowering. It reads **66 unrooted hazards** where
   the curated corpus in the identical mode is gated at **0**.
3. `--debug-symbols` suppresses the failure (0/13). Any instrument that needs
   symbols must use `PERRY_KEEP_SYMBOLS` instead.
4. The fatal frame is `_safeParse`'s inner arrow (`core/parse.ts:65`),
   `result.issues` on an undefined `result` returned by `schema._zod.run(…)`.
   Both observed messages are the same loss one call apart.
5. `core/schemas.ts` closure 138 is both a §12 hazard and a stack frame of a
   failing run.
6. The from-space quarantine suppresses on this workload — confirmed twice now.
   It cannot be the localizing instrument here; `PERRY_UNCAUGHT_BACKTRACE` can.

### Next, in order

1. **Pin closure 138's hazard to a source construct.** It is 1,939 lines of IR;
   the entry names the exact `%r80` and the `js_closure_call1` statepoint.
   Identify which of `core/schemas.ts`'s `run`/`parse`/`runChecks` bodies it is
   and what the unrooted value holds.
2. **Fix the two dominant shapes** (§12): 21× `unrooted:global` (needs a temp
   root — `root_reload.rs` declines these deliberately and correctly) and 16×
   `unrooted:alloc` across `js_array_like_to_array` (no soundness objection,
   just missing). `rooting/temp_root.rs`'s alloca pool is the mechanism; #7719
   is the precedent.
3. **A/B honestly.** At 3/8 on `zod-ks`, a fix needs ~40 clean runs on the SAME
   binary pair to be distinguishable from luck, plus the static count going
   66 → lower. Both, not either.
4. **Gate the cell.** Add `ir-corpus-dep-native` to `gc-root-dominance.yml`
   with a budget that can only go down. Note the corollary CLAUDE.md gives:
   a new gate has never been green, so run it before making it required.
5. **The 12 locale hits** are in never-executed bodies on this workload. Fix
   with the rest; do not use them to judge the fix.

### Loose ends

* `PERRY_GC_SCHEDULE_ALLOC_KB=0` (every poll a candidate, no allocation pacing)
  was identified as a way to make the schedule replayable and was **never
  run** — the backtrace route landed first. It is still the cheapest route to a
  deterministic reproducer if one is wanted.
* Why `-g` suppresses is unexplained (§13).
* The `dyn_eval` frames in §15 mean part of this workload is interpreted.
* Two diagnostics are uncommitted in `wt-7803`:
  `PERRY_UNCAUGHT_BACKTRACE` (`exception.rs`) and `PERRY_KEEP_SYMBOLS`
  (`post_link.rs`). Both are off by default and value-parsed.

## 18. The fix: the CALLEE has to outlive the arguments

Reading the three shapes against the lowering turned them into one defect.

`rooting/temp_root.rs` already answers "root, re-derive, or reuse?" correctly
and in one place (`operand_protection`), and it already says module globals and
locals must be ROOTED rather than reloaded, for the reason a reload gets wrong:

> `new C(g, bump())` where `bump()` sets `g` must capture `g`'s value at call
> time; re-lowering produced the post-`bump()` value, a miscompile rather than
> a rooting bug.

The gap is not the decision, it is the POSITION it is asked about. That
machinery protects call **operands**. Three call-lowering arms lower the
**callee** into a bare register first, lower the arguments after it — each of
which can allocate — and then pass the original register:

```rust
// expr/new_dynamic.rs, both js_new_function_construct arms
let func_double = lower_expr(ctx, callee)?;                       // ← callee
let lowered_args = args.iter().map(|a| lower_expr(ctx, a))…?;     // ← these collect
let (args_ptr, args_len) = lower_js_args_array(ctx, &lowered_args); // ← allocates
ctx.block().call(DOUBLE, "js_new_function_construct",
                 &[(DOUBLE, &func_double), …]);                   // ← pre-move address

// expr/call_spread.rs — same shape, `cb_box` at :458 consumed at :538 across
// the register-buffer stores, `js_array_like_to_array` and the concat.
```

Under the shipping lowering that register is in no statepoint live bundle, so
nothing marks it and nothing relocates it. JS resolves the callee BEFORE it
evaluates the arguments, so the fix has to preserve the call-time value: a temp
root, never a reload. `rooting::RootedGroup` is exactly that and needed no
extension — `adopt` for the hand-emitted callee, `lower` for the arguments,
`reread` below the allocations, one `release` after the consuming call.

### Measured: 66 → 26

Same corpus, same command, `410dadd45` + these two files:

| | before | after |
|---|---|---|
| **total hazards** | **66** | **26** |
| `unrooted/global` | 28 | **5** |
| `unrooted/alloc` | 18 | **2** |
| `unrooted/rootread` | 19 | 19 |
| `unrooted/capture` | 1 | 0 |
| `sink=js_new_function_construct` | 24 | **0** |
| `sink=js_closure_call_apply_with_spread` | 16 | **0** |
| non-empty live bundles | 39,073 | **39,140** |

The two sinks the change targeted are at zero, the `rootread` population it did
NOT target is unchanged at 19, and the corpus grew 67 live bundles — the newly
rooted values entering statepoint bundles, which is what the fix looks like from
the collector's side rather than from the checker's. The unscheduled control run
is byte-identical, so the rooting did not change the answer.

**The residual 19 `unrooted/rootread` are the `js_box_get_bits` shape** — a
mutable-capture box read held across property-get helpers and used as the callee
of `js_closure_call1/2`. That is the shape on §15's failing stack
(`core/schemas.ts` closure 138), and the same callee-outlives-arguments defect
in a third family of arms.

**Fixed too, in `lower_call/early_branches.rs:384`** — and the final number is
better than this section's 26. See §22.

## 19. The fix does NOT close #7803 — the dynamic half says so

Static and dynamic disagree, and the dynamic half is the one that decides.

| binary | codegen | static hazards | sweep |
|---|---|---|---|
| `/tmp/zod-ks` | `410dadd45` | 66 | 3/8 fail |
| `/tmp/zod-fix` | + shapes A, B | **26** | **5/16 fail** (5, 7, 9, 11, 15) |
| `/tmp/zod-fix3` | + shape C | (not measured) | **8/16 fail** (1,3,5,6,8,11,14,16) |

40 hazards closed, two whole sinks to zero, 67 more live bundles — and the
failure rate did not move (3/8 → 5/16 is noise at this sample size). The
messages are unchanged, at the same early safepoints — and `zod-fix3`'s seed 6
adds a THIRD surfacing form of the same loss, `TypeError: is not iterable`.

`zod-fix3`'s 8/16 (50%) against `zod-fix`'s 5/16 (31%) is NOT evidence that
shape C made things worse: Fisher exact gives p≈0.47, i.e. nothing. The rate is
also strongly binary-dependent on this workload with no semantic difference
between builds — 44% / 80% / 38% / 31% / 50% across five binaries of the same
source — and the shape-C edit only moves a *pure* unmask below the argument
lowering. Quoted here so the next reader does not rediscover the 31→50 step and
read it as a regression.

Seed 11 is worth one line on its own: it failed at safepoint **6137** of ~6840,
far later than every other failure (968–2453). Whatever is lost is not confined
to one early window.

**So the three call arms are a real defect that is not this bug.** They are
worth landing on their own terms — the invariant they violate is the one
`docs/src/internals/gc-rooting-invariant.md` states, the fix is the mechanism
the codebase already sanctions, and the corpus count is a ratchet — but #7803
stays open and the cause is elsewhere.

Recording the negative result at full strength because the temptation here is
real: a 66 → 26 table looks like progress on the issue, and quoting it without
the sweep beside it would have been the exact "gate that cannot fail" shape
CLAUDE.md warns about, one level up — a *measurement* that cannot fail, because
its subject was never the failure.

### Where the evidence now points: the `dyn_eval` interpreter

§15's stack has frames the static checker structurally cannot see:

```
 8  dyn_eval::expr::eval_expr
 9  dyn_eval::interp::exec_stmt
10  dyn_eval::interp::interp_thunk
11  closure::registry::dispatch_with_arity
```

Part of `zod` executes through the V8-fallback INTERPRETER, not as native code.
`scripts/gc_root_dominance_check.py` reads emitted LLVM IR, so an interpreter
written in Rust is invisible to it — every hazard it can report is in a
population that, on this workload, may not contain the bug at all. That is
CLAUDE.md's own warning, and it fits every observation:

* fixing 40 IR-level hazards changed nothing;
* the quarantine suppresses (§3, §16) — recycling decides what a stale read
  finds, whoever holds the stale pointer;
* `--debug-symbols` suppresses (§13) — a layout/inlining sensitivity, not
  something a rooting fix in emitted IR would move.

`dyn_eval` DOES have a root scanner (`scan_dyn_eval_roots_mut`, `ROOTS`
thread-local, plus the env/member key caches), so the question is not "are
there roots" but **"does every interpreter-held JSValue reach `ROOTS` before a
call that can collect"** — an `f64` local in `eval_expr` held across a call into
user code is exactly the shape, and it is the intermittent-register kind rather
than the reproducible-table kind.

**Next investigator: audit `dyn_eval/expr.rs` and `dyn_eval/interp.rs` for
`f64`/JSValue locals live across calls that can collect, before spending more
on the IR corpus.** And find out WHY part of this workload is interpreted at
all — a natively compiled `zod` would not use that path (#678 is the tracker
for native callsites into V8-fallback modules).

> **Not yet done for the three call arms**: the gap suite has NOT been run
> against them. They change the lowering of every `new <expr>(…)`, every spread
> call and every closure-typed-local call in the language, so `./scripts/
> run_gap_tests.sh` plus `cargo test -p perry-codegen` gate any PR — the
> unscheduled dep-corpus control run being byte-identical is nowhere near
> sufficient evidence for a change with that blast radius.

## 20. THE PATH IS THE `new Function` INTERPRETER — one experiment, not an audit

Instead of auditing `dyn_eval`, take the path out of the workload and see if the
bug leaves with it.

### Why the workload interprets at all

`zod/src/v4/core/schemas.ts:2028`: for **every object schema**, zod builds a
"fastpass" parser by generating source and compiling it with `new Function`
(`doc.compile()`), then routes `parse` through it. On Perry `new Function` lands
in the `dyn_eval` interpreter. The corpus's `parseLoop(96)` parses object
schemas 96 times, so the failing path runs generated code every iteration —
which is why §15's stack has `interp_thunk` two frames under the throw.

zod ships the switch: `core.globalConfig.jitless` makes `parse` fall through to
`superParse`, all natively compiled.

### Two things that had to be got right first

**The config has to run before any schema is built.** `const jit =
!core.globalConfig.jitless` is captured when the `$ZodObject` is CONSTRUCTED
(schemas.ts:2007), and `alerts.ts` / `orgs.ts` / `scans.ts` build schemas at
import time — before `main.ts`'s body. A `z.config(...)` at the top of `main`'s
body is already too late. It moved into `jitless-first.ts`, imported ahead of
the schema modules.

**The subject has to be asserted absent, not assumed absent.** The first
attempt *looked* right and was not: `/tmp/zod-jitless` still entered
`interp_thunk` through the identical `core/schemas.ts` 138 → 137 →
`js_closure_call3` stack as the failing run. Had it been swept as-is, a clean
result would have been quoted as "jitless is clean" while the interpreter was
still running the parse.

The check that settles it, on the corrected build — armed breakpoint, whole
program, no hit:

```
lldb -b -o 'breakpoint set -r interp_thunk' -o run -- /tmp/zod-jitless2
→ endpoints=9 … Process exited with status = 0    (never stopped)
```

(`dyn_function_from_strings` IS still reached in both builds — zod's
`util.allowsEval` probe compiles `new Function("return true")` regardless. So
"does `new Function` appear" is the wrong question; "does the parse path
INTERPRET" is the right one, and `interp_thunk` is what answers it.)

### The result

`RATE=1 PERRY_GC_PROTECT_FROMSPACE=0`, seeds 1..16, same compiler, same runtime
archives, same `zod@4.3.5`:

| binary | interpreter on the parse path | sweep |
|---|---|---|
| `/tmp/zod-ks` | yes | 3/8 fail |
| `/tmp/zod-fix3` | yes | 8/16 fail |
| **`/tmp/zod-jitless2`** | **no** | **0/16** |

At the jit builds' rate (31–50%), sixteen consecutive passes is p ≈ 0.001 at
37.5%, and every one of them ran the instrument hot: 5,054–5,434 forced
collections, ~765k objects moved per run. The answer is byte-identical
(`endpoints=9 parsed=96 registered=9`) — the schemas still parse, they just
parse natively.

### What this is, and what it is not

It is strong evidence that **the lost value lives on the generated-code /
`dyn_eval` path**, and it explains every earlier result at once: why 40 IR-level
hazards closed with no effect (the checker reads emitted LLVM IR and the
interpreter is Rust), why the quarantine suppresses, and why a build-layout
change like `-g` moves it.

It is NOT a clean single-variable A/B and must not be quoted as one. `jitless`
changes the workload: 5,056 safepoints against 6,840, ~26% fewer collections and
a different allocation profile. A workload that collects less can fail less for
reasons that have nothing to do with who holds the pointer. What makes it
persuasive is the CONJUNCTION with §15's stack, not the sweep alone.

The way to close that gap is not another sweep — it is to fix the interpreter's
rooting and show the *jit* build go green, which is the same evidence with the
confound removed.

### Next

Audit `dyn_eval` for JSValues held across calls that can collect —
`interp::exec_stmt`, `expr::eval_expr`, and `closure::registry::
dispatch_with_arity` (all three on the failing stack). `scan_dyn_eval_roots_mut`
already scans a `ROOTS` thread-local plus the env/member key caches, so the
question is not whether roots exist but whether every intermediate reaches them
before a call — an `f64` local in `eval_expr` across a user call is exactly the
shape, and it is the intermittent-register kind, not the reproducible-table
kind.

Second question, worth its own issue: **why does a compile-as-package build
interpret zod's hot parse path at all?** `Doc.compile`'s generated source is
known at build time for a static schema; #678 tracks native callsites into
V8-fallback modules. That is a performance finding independent of this bug.

## 21. The architectural finding: the interpreter had no GC safepoints at all

Auditing `dyn_eval` by hand first, because a fix needs a defect and I had a
hypothesis rather than one. The interpreter's rooting discipline is **better
than expected** — the hazardous shapes are all handled:

* `eval_binary` roots the LHS before evaluating the RHS and re-reads both from
  `roots` afterwards;
* `eval_call` roots the receiver before `eval_args`, re-reads it for the
  dispatch, and `eval_args` roots every argument as it is produced;
* `set_prop_by_name` roots the VALUE before evaluating a computed key;
* `js_native_call_method`, the bridge's dispatch target, opens a
  `RuntimeHandleScope`, roots receiver and args on entry, and (#7528) re-reads
  them per use rather than once at the top.

A mechanical scan for "value produced, used below an intervening call" over all
of `expr.rs` / `interp.rs` / `bridge.rs` / `env.rs` returned ~40 candidates and
every one I checked was a non-allocating probe (`truthy`, `to_number` on a
number) or already rooted. **I did not find the hole by reading.**

### What I found instead

The interpreter offers the collector **no cooperative safepoints whatsoever**.
Compiled code polls at loop back-edges (default on since #7721). Interpreted
code polls nowhere, so a collection can only reach it at an *allocation* point
— and the alloc-point arm forces a conservative stack scan, which finds Rust
locals and makes the copying minor ineligible.

The consequence is not that the interpreter is safe. It is that the
interpreter is **untestable**:

| instrument | reaches compiled code | reaches `dyn_eval` |
|---|---|---|
| `gc_root_dominance_check.py` (3 modes) | yes | no — there is no IR |
| `PERRY_GC_ZEAL` | yes, at back-edge polls | **no — no safepoints** |
| `PERRY_GC_SCHEDULE_SEED` | yes | **no — no safepoints** |
| `PERRY_GC_PROTECT_FROMSPACE` | yes | only via compiled frames |

So the one rooting domain with no static checker also had no dynamic one, and
`mod.rs`'s claim — "interpreter frames hold **every** live JSValue in a rooted
thread-local value stack" — was unfalsifiable by anything in the tree. That is
the architectural defect, independent of what #7803's own root cause turns out
to be.

### `PERRY_GC_INTERP_SAFEPOINTS=1`

`dyn_eval::interp_safepoint()`, called at every `eval_expr` node and every
`exec_stmt`. It routes through `js_gc_loop_safepoint` deliberately rather than
collecting directly, so every entry guard (in-alloc, root-lock, unsafe-FFI
zone, budgeted cycle) and the seeded-schedule ordinal apply exactly as they do
to a compiled back-edge: an interpreter safepoint is the *same* safepoint, not
a second kind. Both existing instruments now reach the interpreter for free.

**Subject asserted live** — seed 2, rate 1, same binary:

| | `loop_polls` | safepoints | moved |
|---|---|---|---|
| off | 24,029 | 2,725 | 369,076 |
| **on** | **93,210** | **6,973** | **866,480** |

69,181 additional polls, ~4× the compiled ones. That number IS the size of the
blind spot: on this workload the interpreter was where most of the potential
safepoints were, and none of them existed.

Output is byte-identical in both modes.

### Why it is opt-in and not on

If the interpreter's rooting is complete, default-on is strictly better — the
copying minor becomes eligible where only a conservative sweep could run. If it
is not, flipping it turns a latent hole into a live crash for exactly the
workloads `dyn_eval` exists to serve (ajv, fast-json-stringify, find-my-way,
every fastify app). Shipping that before the rooting is verified trades a quiet
bug for a loud one in someone else's server. So it lands as an instrument, and
the flip is a separate evidence-gated decision — the same sequencing
`PERRY_GC_MOVING_LOOP_POLLS` had between #7161 and #7721.

### 21a. The sweeper took the build mid-session — commits survived

§8 recorded that "whatever sweeps `/Users/amlug/projects/perry/wt-*` on this
box" deleted the previous session's worktree AND its `CARGO_TARGET_DIR` while
an experiment was running. It happened again here: `wt-7803/target/` vanished
between two commands (free space 8 GB → 60 GB), taking `perry`,
`libperry_runtime.a` and `libperry_stdlib.a` with it.

Nothing was lost, because the work had been committed as it was produced —
five commits, all intact, plus the uncommitted working-tree edits. The
in-flight A/B kept running because its binary lives in `/tmp`, not the
worktree.

**Operational rule for this box, stated because it has now cost two sessions:**
commit before any long-running step, and never treat a worktree `target/` as
durable for longer than a single command. The 25 minutes to rebuild is the
whole cost when you have commits; it is the whole session when you don't.

## 22. Final static number: 66 → 3, and a lesson about which build you measured

The third arm (`lower_call/early_branches.rs`: `recv_box` lowered, arguments
lowered, then unmasked into `closure_handle`) was fixed but never measured
statically — the 26 in §18 was taken from a corpus emitted before that fix
existed. On a CLEAN rebuild with all three arms:

```
=== statepoint hazards: 3  (unrooted: 3, stale: 0)
       2  unrooted/alloc     1  unrooted/rootread
       1  sink=js_array_concat
       1  sink=js_rel_ge
       1  sink=js_get_string_pointer_unified
```

| sink | before | after |
|---|---|---|
| `js_new_function_construct` | 24 | **0** |
| `js_closure_call_apply_with_spread` | 16 | **0** |
| `js_closure_call1` / `js_closure_call2` | 23 | **0** |
| everything else | 3 | 3 |
| **total** | **66** | **3** |

Live bundles 39,073 → 39,186; relocations 444,472. The dependency corpus under
the shipping lowering is now within a hair of the zero its curated sibling is
gated at.

> **The lesson is about the 26, not the 3.** That number came from an
> incremental build whose corpus predated one of the three fixes, and it went
> into a committed gate budget. A ratchet's number has to come from a tree
> someone else can reproduce — a clean build — or the ratchet encodes whatever
> the build directory happened to contain that afternoon. Caught only because
> the worktree was swept and the rebuild was from scratch; a friendlier box
> would have shipped `--max-unrooted 26` and never known.

The gate now carries `--max-unrooted 3 --max-stale 0`.

**None of this closes #7803** (§19): the failure rate is unmoved. Two separate
true statements, and the second one is the one the issue is about.

## 23. The interpreter-safepoint A/B, which points AWAY from the obvious reading

One binary, one variable, quarantine off, seeds 1–8:

| `PERRY_GC_INTERP_SAFEPOINTS` | failures |
|---|---|
| off | **6/8** |
| on | **2/8** |

Collecting *more* often inside the interpreter made the workload fail *less*.
That is the opposite of what "the interpreter holds the unrooted value"
predicts — if interpreted frames were the hazard, adding ~69,000 collection
opportunities inside them should have raised the rate, not halved it.

n=8 and p≈0.13, so it settles nothing on its own. But taken with §20 it means
the honest position is narrower than "the interpreter is the culprit":

* §20 shows the failure needs the `new Function` PATH (0/16 without it);
* §23 shows that collecting inside the interpreter does not make it worse.

Both can hold if the lost value is not held by the interpreter at all but by
something the interpreted path *reaches* — the bridge between the two worlds,
or a compiled callee invoked from interpreted code, or a runtime cache keyed on
a value the interpreter passed. Note §15's stack crosses that boundary twice
(`js_native_call_method` → `dispatch_with_arity` → `interp_thunk` → back out
through `js_native_call_method`), and CLAUDE.md's own warning applies to the
runtime side of it: a thread-local or side table holding a `*mut` into the heap
is invisible to the static checker.

**So the next investigator should not start by auditing `dyn_eval`'s own
locals** — §21 already did that and found the discipline sound. Start at the
BOUNDARY: `dyn_eval/bridge.rs`, `closure::registry::dispatch_with_arity`, and
whatever caches the interpreted-closure dispatch path populates.

## 24. What is verified, what is not, and the one thing blocking the codegen PR

### Verified in this session

| claim | how |
|---|---|
| #7803 live on `410dadd45` | 7/16, 8/16, 6/8 across builds |
| fatal frame is `parse.ts:65` | symbolicated native backtrace, §14 |
| both messages are one loss | same stack, two surfacing points, §14/§15 |
| the failure needs the `new Function` path | 0/16 jitless vs 8/16, instrument hot, §20 |
| the dep corpus × native lowering was ungated | four-cell matrix, §9 |
| that cell read 66 unrooted, curated reads 0 | §9a, gate green on main |
| three call arms lose the callee | source read + 66→3 after the fix, §18/§22 |
| the interpreter had no safepoints | `loop_polls` 24,029 → 93,210, §21 |
| more interpreter collection does NOT worsen it | 6/8 → 2/8, §23 |
| `--debug-symbols` suppresses it | 0/13 vs 44%, §13 |
| the quarantine suppresses it | seed 3 fails unprotected, passes protected, §16 |

### NOT verified — and the codegen change must not land until it is

**The gap suite has not run against the three call arms.** They change the
lowering of every `new <expr>(…)`, every spread call and every closure-typed
local call in the language. This box could not give a trustworthy run: load
average **60** with 47 sibling worktrees building, and the suite went from 25
tests in 3 minutes to 30 in 19. A timeout-flake red under that load is worse
than no run, so it was stopped rather than finished badly.

**Partial: 30/554, 0 failures** (`scratchpad/zod/gap-partial.log`). That is
evidence of nothing except that the first 30 do not crash.

Before the codegen commit (`95d9fbb9d` + the `early_branches.rs` arm) goes into
a PR: `./scripts/run_gap_tests.sh` and `cargo test -p perry-codegen`, on a
quiet host.

### Still open, and where to look next

The cause. §23 narrows it: the failure needs the interpreted path, but the
interpreter's own frames are neither obviously the holder (§21's audit) nor
made worse by collecting in them (§23). The remaining surface is the
**boundary** — `dyn_eval/bridge.rs`, `closure::registry::dispatch_with_arity`,
and any runtime cache the interpreted-dispatch path populates. A runtime-side
cache of a raw heap pointer is invisible to the static checker by construction,
and CLAUDE.md's rule of thumb applies in reverse here: this bug is intermittent,
which argues for a register rather than a table — but a table reached only from
the interpreted path would also present intermittently, because the path itself
is only taken 96 times.

## 25. At the boundary: `js_native_call_method` hands some callees a STALE argument buffer

§23 said to look at the boundary rather than at `dyn_eval`'s own locals. Doing
that found a defect of exactly the right shape, in the frame that is literally
on the failing stack (`js_native_call_method`, frame 7 of §15).

#7528 established the rule for this function and stated it well:

> `object_handle` roots the receiver, but a value READ OUT of a root and held
> in a local is not rooted — the collector rewrites the SLOT, not the copy.
> This function then runs ~1160 more lines across a dozen probes that allocate.

Its fix was `refreshed_args()` — re-read the rooted arguments at the point of
use. **It reaches ten sites. The function has many more dispatch arms, and
several of them pass the caller's raw `args_ptr` instead.** That buffer is the
CALLER's memory; `arg_handles` is what the collector rewrites. Nobody rewrites
the buffer.

Two arms verified to have a collection point between entry and the dispatch:

```rust
// ~1424, dynamic prop on a closure receiver
let bound = clone_closure_rebind_this(dyn_val.to_bits(), object());  // ALLOCATES
js_native_call_value(f64::from_bits(bound), args_ptr, args_len);     // ← stale buffer

// ~1476, accessor getter
let method_fn = js_closure_call0(getter);                 // runs USER CODE
let bound = clone_closure_rebind_this(method_fn.to_bits(), object());  // ALLOCATES
js_native_call_value(f64::from_bits(bound), args_ptr, args_len);       // ← stale buffer
```

Both now use `refreshed_args()`.

**Why this fits #7803's symptom exactly.** zod's generated fastpass calls
`shape[k]._zod.run({ value: input[k], issues: [] }, ctx)` — the first argument
is a freshly allocated object literal, the youngest possible object, the one
most likely to be moved by the next minor. If the tower collects between entry
and dispatch, the callee is handed the pre-move address of that literal:
`result` comes back wrong, and the caller reads `.issues` on it. That is the
message, on the argument that literally contains `issues: []`.

And `_zod` is an ACCESSOR on zod's schema objects, which is the second arm.

**MEASURED, and it does NOT close #7803.** `/tmp/zod-argfix`, same conditions:
seeds 1, 2, 3, 5 pass — and **seed 4 fails** with `value is not a function` at
safepoint 765. One failure is enough; a fix would need a clean sweep. So this
section records a real defect found and fixed, and a cause REFUTED. (The rate
over the full 16 is still running and only affects how *much* it helped, not
whether it closed it.) The remaining raw-`args_ptr` arms
(the JS-handle dispatcher at ~1496 and several others) were left alone: they
need the same per-arm "can anything above me collect?" argument, and guessing
uniformly would be the audit-by-eye that §21 already showed is unreliable.

## 26. Where this session ends, and the two things blocked on a quiet host

Everything below is committed on `fix/7803-zod-gc-rooting` (10 commits).

### Landed

| | what | evidence |
|---|---|---|
| diagnostics | `PERRY_UNCAUGHT_BACKTRACE`, `PERRY_KEEP_SYMBOLS` | §11, §13 — the pair that made §14's localization possible at all |
| codegen | callee rooted across argument evaluation, 3 arms | **66 → 3** hazards, §18/§22 |
| runtime | `dyn_eval` cooperative safepoints | `loop_polls` 24,029 → 93,210, §21 |
| runtime | argument buffer refreshed in 2 dispatch arms | §25 — fits the symptom precisely, and does NOT close the bug (seed 4 still fails) |
| CI | the fourth corpus × lowering cell, gated at 3 | §9, §22 — verified end to end |

### Blocked on host load, not on work

The box sat at load **40–74** with 47–49 sibling worktrees building for the
last several hours. Two verifications need a quiet host and are the only thing
between this branch and a PR:

1. **`./scripts/run_gap_tests.sh` + `cargo test -p perry-codegen`** for the
   three call arms, which change the lowering of every `new`, spread call and
   closure-typed-local call in the language. Partial run: 30/554, 0 failures,
   stopped when the suite slowed from 25-tests-in-3-minutes to 30-in-19.
2. ~~The rate A/B for §25's fix~~ — **answered**: seed 4 fails, so it does not
   close the bug. The remaining seeds only refine the rate.

Neither is a judgement call. Both are "run this on a machine that isn't at
load 60".

### The §25 follow-up stands regardless of it not being the cause

The follow-up is not "fix the other arms one at a time". `js_native_call_method`
has one rule — *no value read out of a root may be used below a collection
point* — and enforces it by two different means in the same function: ten sites
call `refreshed_args()`, the rest pass the caller's raw buffer, and nothing
distinguishes them but an author's per-arm judgement. That is the same
"invariant maintained by audit" shape as `dyn_eval`'s `root_push` discipline
and as the pre-`RootedGroup` codegen. The architectural fix is to make the raw
buffer unreachable from the dispatch arms — hand them a type that can only
yield refreshed values — so the losing spelling stops compiling.

### Where the cause still hides

The remaining surface, in order: the other raw-`args_ptr` arms (~1496 and
below), `dyn_eval/bridge.rs`, and any runtime cache the interpreted-dispatch
path populates. §23's A/B says the interpreter's own frames are not obviously
the holder, and §21's audit says its `root_push` discipline is sound, so the
boundary remains the place to look.

## 27. Scorecard: four fixes, four times the bug survived

| # | fix | static effect | effect on #7803 |
|---|---|---|---|
| §18 | callee rooted, `new_dynamic.rs` ×2 + `call_spread.rs` | 66 → 26 | none (5/16) |
| §22 | callee rooted, `early_branches.rs` | 26 → 3 | none (8/16) |
| §21 | `dyn_eval` cooperative safepoints | — | rate *fell* 6/8 → 2/8, bug survives |
| §25 | argument buffer refreshed, 2 dispatch arms | — | none (seed 4 fails) |

Four separate rooting defects, all real, all in the right family, none of them
this bug. That is worth stating as its own finding: **the zod corpus under a
rate-1 unprotected schedule is not a one-defect workload.** Each fix was
justified on its own evidence and each left the failure standing.

The discipline that made this readable is the one to keep: every fix got its
own A/B against the SAME binary pair, and every null result was written down at
full strength instead of being folded into the next attempt's baseline. The
alternative — landing four fixes and re-measuring once at the end — would have
produced a single ambiguous number and no way to attribute it.

What is now known about the cause, positively:

* it needs the `new Function` / interpreted path (§20, 0/16 without it);
* it is not in `dyn_eval`'s own `root_push` discipline (§21 audit) and not made
  worse by collecting there (§23);
* it survives every callee- and argument-rooting fix in the compiled tower and
  in the three call lowering arms (§27, this table);
* it is suppressed by `--debug-symbols` (§13) and by the from-space quarantine
  (§16), which are both *layout* interventions rather than rooting ones.

That last line is the one I would pull on next. Two independent interventions
that change memory LAYOUT (not rooting) both make it vanish, while four
interventions that change ROOTING leave it untouched. That pattern fits a stale
raw pointer held somewhere the collector never rewrites — a runtime-side cache
keyed on an address, rather than a value on anyone's stack. CLAUDE.md names
that class and says the static checker cannot see it; the registry to audit is
`gc_register_mutable_root_scanner`'s ~123 entries, and the ones reached only
from the interpreted-dispatch path are the short list.
