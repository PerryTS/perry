# Task: close GitHub issue #7803 (Perry GC rooting bug on the zod dep corpus)

This file is a self-contained brief. It assumes no memory of prior sessions.
Background detail lives in `gc-handoff/ZOD-NOTES.md` (~1000 lines, sections
numbered §1–§33); this brief tells you what to do and cites sections for the
"why". **Read §32 and §33 before starting** — they say what has already been
ruled out, and repeating it is the main way to waste a day here.

---

## 0. The bug in five lines

`test-files/gc-dep-corpus/main.ts` (a 4-file harness plus the real `zod@4.3.5`
compiled from source) dies ~30–50% of the time under a seeded GC schedule with
the from-space quarantine off. Three surfacing messages, all one defect seen at
different points: `Cannot read properties of undefined (reading 'issues')`,
`value is not a function`, `is not iterable`. The fatal frame is
`node_modules/zod/src/v4/core/parse.ts:65` — `result.issues` where
`schema._zod.run({ value, issues: [] }, ctx)` returned `undefined` (§14). The
failure needs the `new Function` path, which on Perry runs through the
`dyn_eval` tree-walking interpreter (§20).

**Root cause is UNKNOWN.** Five hypotheses have been tested and refuted or
left unsupported (§32). Two real rooting defects were found and fixed along the
way; neither closed the bug.

---

## 1. Acceptance bar — what "fixed" means

All three. The first is non-negotiable.

1. **A named root cause**: which value was lost, held by what, across which
   collection. A rate improvement alone is NOT acceptable evidence — five
   separate interventions moved the rate without being the cause, so "the
   number got better" is the failure mode this bug specialises in.
2. **A deterministic reproducer that flips**: a specific seed under the pinned
   schedule (§2.4) that FAILS before your fix and PASSES after, same binary
   pair, plus the counters showing the collector actually ran.
3. **A sabotage test**: re-introduce the defect, show the failure returns, and
   land a CI regression test that asserts its own subject ran (a test that
   passes because nothing executed is this repo's most-repeated own-goal — see
   CLAUDE.md "★ Four ways a gate can be unable to fail").

Fallback if no deterministic reproducer emerges: 0 failures in 40 runs on the
pinned config (p≈1e-9 at a 40% base rate). This is ~20 machine-hours per arm and
still does not identify the cause, so treat it as a last resort, not the plan.

---

## 2. Environment

### 2.1 Worktree

```
/Users/amlug/projects/perry/wt-7803    branch fix/7803-zod-gc-rooting
```
22 commits ahead of `410dadd45`. Everything below assumes `cd` there.

### 2.2 Host hazards — read these, they each cost this session real time

* **Something sweeps `/Users/amlug/projects/perry/wt-*` and deletes `target/`
  mid-session.** It happened twice. **Commit before any long step.** Build
  output is never durable for longer than one command (§21a).
* **The box is shared with other agents.** Load hit 74 with 49 sibling
  worktrees. Check `uptime` before any timing-sensitive run; anything above
  ~20 makes wall-clock numbers meaningless and slows a gap run 6×.
* **Check free disk before starting** (`df -h /`). This session hit ENOSPC
  twice; a `PERRY_GC_DIAG=1` run writes tens of MB, and the quarantine at
  depth 800 holds ~7 GB of RAM.
* **Never run a `cargo build` concurrently with a test suite.** It swaps
  `target/release/perry` mid-run and produces spurious failures — that cost a
  false `compile_fail` in the gap suite (§29).

### 2.3 Build

```bash
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
```
All three packages, every time — `perry-runtime`/`perry-stdlib` are rlib-only
and the `.a` files come from the `-static` wrappers. Building the wrong set
links a STALE archive and both arms of an A/B behave identically (CLAUDE.md
"Verifying a runtime change"). ~8–16 min. Confirm the `.a` mtimes moved after
your edit.

### 2.4 Compile the corpus

```bash
PERRY_RUNTIME_DIR=$PWD/target/release \
PERRY_NO_AUTO_OPTIMIZE=1 \
PERRY_DISABLE_BUILD_CACHE=1 \
PERRY_KEEP_SYMBOLS=1 \
  ./target/release/perry test-files/gc-dep-corpus/main.ts -o /tmp/zod
```
~5 min quiet, ~15 loaded. Expected output when run:
```
endpoints=9
parsed=96
registered=9
GET https://registry.example.com/v1/alerts [alerts|read|get|abs] {-} #2
```

* `PERRY_NO_AUTO_OPTIMIZE=1` is **not optional** — without it the auto-optimizer
  relinks the runtime without the `diagnostics` feature and silently removes the
  instrumentation you are about to rely on.
* `PERRY_KEEP_SYMBOLS=1` keeps the symbol table WITHOUT `-g`. Use this, never
  `--debug-symbols`: **`--debug-symbols` suppresses the bug** (0/13 vs 44%,
  §13). This is the single most important trap in this file.

### 2.5 The reproducer

```bash
PERRY_GC_SCHEDULE_SEED=$N \
PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=0 \
PERRY_UNCAUGHT_BACKTRACE=1 \
  /tmp/zod
```
Exit 0 = pass. Exit 1 with a `TypeError` on stderr = the bug. Add
`PERRY_GC_SCHEDULE_ALLOC_KB=0` for the **pinned** schedule: it removes the
allocation pacing so the candidate set is every loop poll, giving
`safepoints=63941 scheduled_collections=63941 polls_paced=0` — reproduced to
the digit across runs (§31). Without it the schedule drifts ~1–4% run to run,
which is the same magnitude as the effects you are trying to measure, and no
6-to-16-run sweep can attribute anything (§30). Cost: ~9.4× the collections,
30–60 min per run.

Always confirm the instrument was live by reading the exit line:
```
[gc-schedule] done: seed=N safepoints=… copying_minors=… moved_objects=… loop_polls=…
```
`copying_minors=0` or `moved_objects=0` means the run tested nothing.

### 2.6 Diagnostics available (all default-off, all parsed BY VALUE)

| knob | what it does | §|
|---|---|---|
| `PERRY_UNCAUGHT_BACKTRACE=1` | symbolicated native backtrace at the uncaught throw | §11 |
| `PERRY_KEEP_SYMBOLS=1` | skip the final strip, no `-g` | §13 |
| `PERRY_GC_INTERP_SAFEPOINTS=1` | give `dyn_eval` cooperative GC safepoints | §21 |
| `PERRY_GC_POISON_FROMSPACE=1` | layout-neutral poison of retired from-space | §30 |
| `PERRY_GC_TENURING_SURVIVALS=<u8>` | pin the promotion age | §32 |

Pre-existing and relevant: `PERRY_GC_PROTECT_FROMSPACE=1` +
`PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` (quarantine), `PERRY_GC_ZEAL=1`,
`PERRY_GC_VERIFY_EVACUATION=1`, `PERRY_GC_FROMSPACE_SCAN=1`, `PERRY_GC_DIAG=1`.

---

## 3. Do NOT re-derive these

| already tested | result | § |
|---|---|---|
| `Object.defineProperty/Properties` rooting | refuted by sabotage A/B | §2 |
| callee unrooted across arguments (compiled codegen, 3 arms) | real defect, FIXED, did not close the bug | §18–§19, §22 |
| `dyn_eval`'s own `root_push` discipline | audited sound; more collection there made it *better* | §21, §23 |
| stale argument buffer, 2 dispatch arms | real defect, FIXED, did not close the bug (6/16) | §25 |
| promotion / tenuring age | not supported; non-monotonic | §32 |
| handle-band address predicate | dead — the band is 1 MB, heap is far above | §32 area |

**Known or suspected suppressors** — if the bug "disappears", check you have not
enabled one of these by accident:

* `--debug-symbols` — **established** (0/13 vs 44%), §13
* the from-space quarantine — **established twice**, §3/§16
* `PERRY_GC_INTERP_SAFEPOINTS=1` (6/8 → 2/8), `PERRY_GC_POISON_FROMSPACE=1`
  (3/6 → 0/6), `PERRY_GC_TENURING_SURVIVALS=255` (0/5) — all **under-powered**
  at n≤8; do not treat as facts, and do not build on them without more runs.

---

## 4. The plan

### Step 1 (the unlock) — find a deterministic failing seed

Everything else is cheap once this exists, and expensive while it does not.

```bash
# unattended; parallelise across seeds, but keep total load under ~10
for s in $(seq 1 40); do
  PERRY_GC_SCHEDULE_SEED=$s PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=0 \
  PERRY_UNCAUGHT_BACKTRACE=1 timeout 5400 /tmp/zod >o.$s 2>e.$s
  echo "seed $s exit=$?"
done
```
Seed 1 is known to PASS under the pinned config. When a seed fails, **run it
twice more** to confirm it is deterministic; the whole point is a reproducer you
can A/B against.

If a wide sweep finds nothing, that is itself a result: the failure needs the
*paced* feedback loop, and Step 1 becomes "find a paced seed that fails 3/3",
which is weaker but still usable.

### Step 2 — catch it in the act

With a deterministic failing seed, the instrument shelf becomes usable for the
first time. In order of directness:

1. `PERRY_GC_VERIFY_EVACUATION=1` — panics if a live mutable slot still points
   at a forwarded nursery object.
2. `PERRY_GC_FROMSPACE_SCAN=1` (implies the abort variant) — whole-heap scan
   after the rewrite pass; reports owner, slot offset, target `obj_type`, and
   remembered-set coverage of the offending slot.
3. Quarantine at `DEPTH=800` **on the failing seed only** — it suppresses in
   aggregate, but on a known-failing cycle a stale deref faults precisely.
4. `PERRY_GC_POISON_FROMSPACE=1` — layout-neutral, so a stale read returns the
   poison word instead of a plausible object.

Expected product: an owner, a slot, and an `obj_type`. That is the named root
cause the acceptance bar wants.

### Step 3 — fix, then prove it

Fix the named cause. Then, in this order:
1. the failing seed passes, same binary pair;
2. sabotage: revert the fix, failure returns;
3. regression test that asserts its subject ran;
4. `./scripts/run_gap_tests.sh` and `cargo test -p perry-codegen` on a QUIET
   host (needs Node 26.5.1 per `.node-version`, and `npm ci --ignore-scripts
   --no-audit --no-fund` — six gap tests fail without their npm packages, §29).

### Step 4 (independent, do it regardless) — the 36-site population

`js_native_call_method` passes the caller's raw `args_ptr` below its handle
scope at **36 sites**; #7528 converted 10 of them and the other 26 were never
distinguished from those by anything but per-arm judgement (§33). The file's own
rationale — the receiver is re-read at every use because "this function then
runs ~1160 more lines across a dozen probes that allocate" — applies verbatim
to the arguments.

Fix: a **per-site** `let ra = refreshed_args();` (a single refresh at the top is
the exact mistake #7528 documents). Land it WITH the enforcement so the
population cannot regrow:
```rust
// immediately after arg_handles is built
#[allow(unused_variables)]
let args_ptr = ();
#[allow(unused_variables)]
let args_len = ();
```
`cargo check` then errors on exactly the sites that still reach for the stale
buffer. The hot path does not pay: `try_class_vtable_fast_dispatch` returns
above the scope, so all 36 are already slow paths.

This is worth landing whether or not it is the bug. Do NOT claim it closes
#7803 without Step 1's reproducer.

---

## 5. Method rules for this bug specifically

1. **Never conclude from a 5-to-16-run sweep.** At a ~40% base rate that cannot
   distinguish 30% from 50%. Say "under-powered" and move on (§30).
2. **A rate change is not a cause.** Five interventions moved it; none was the
   cause. State the mechanism or state that you do not have one.
3. **Change one thing.** This session blamed lldb for a suppression that was
   actually `--debug-symbols`, because two variables moved at once (§13).
4. **Assert the subject ran** before believing a green: read `copying_minors`,
   `moved_objects`, `loop_polls`. A clean sweep of a run that collected nothing
   is the house speciality.
5. **Check exit codes, not output.** Several gap "failures" are the ORACLE
   failing (`Node exit: 1` — missing npm packages, or `enum`/parameter
   properties that `--experimental-strip-types` cannot strip). Perry was right
   in every one (§28–§29).
6. **Write down null results at full strength.** Four of this session's most
   useful findings are refutations. Folding a null into the next attempt's
   baseline is how you end up with one ambiguous number and no attribution.

---

## 6. Deliverables

* Root cause named, with the slot/owner evidence.
* Fix + deterministic-reproducer flip + sabotage test + CI regression test.
* Step 4 landed (separately reviewable, not claimed as the fix).
* `gc-handoff/ZOD-NOTES.md` extended with what you did, including what failed.
* Version bump + `changelog.d/<PR>-<slug>.md` per CLAUDE.md; no `CHANGELOG.md`
  edit, no version bump on a contributor PR.

## 7. Scoping note

#7803's title still names "seed 1, safepoint 3319", a reproducer stale since
session 1. Consider retitling to the durable statement (the corpus fails ~40%
under a rate-1 unprotected schedule, localized to `parse.ts:65`, needs the
`new Function` path) and splitting Step 4 into its own issue so it is not
blocked on this mystery.
