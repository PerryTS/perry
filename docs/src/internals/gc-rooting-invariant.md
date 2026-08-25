# The GC rooting invariant (codegen)

Read this before you emit a call from a lowering.

## The rule

> **Any GC-managed value that is live across a collection point must be
> reachable from a root before that point.**
>
> A value read out of a root and held in an SSA register across a call **is not
> rooted**. It is a copy, and the collector cannot see copies.

Perry's GC moves objects. When an evacuating minor runs, it walks the roots,
copies live objects to old-gen, and **rewrites every reference it can reach**.
Anything it cannot reach keeps the old address. That address now points into
from-space, which is about to be reused.

A "collection point" is any of:

- an allocation (`js_object_alloc`, `js_array_alloc`, `js_closure_alloc`, string
  concatenation, boxing — anything that can take an arena block);
- a call that can allocate, which in practice means **almost every runtime
  helper**. `js_object_set_field_by_name` allocates: it performs the keys-array
  transition. `js_object_get_property` allocates: it can run a getter, which is
  user code;
- `js_gc_loop_safepoint`, the back-edge poll (only emitted under
  `PERRY_GC_MOVING_LOOP_POLLS`, ON by default again, kill switch `=0`);
- `js_gc_collect` — a JS-level `gc()`. Since #7558 this runs a full mark-sweep
  on **precise roots** like everything else, so a value live across it and not
  reachable from a root is *freed*. It used to force the conservative
  native-stack scan (#4977), which hid exactly this shape; it does not any more.
  Note that a `gc()`-only window is invisible to `--moving-only`, because a full
  mark-sweep frees rather than moves — check such a function without that flag.

The safe default is that **a call collects unless you have read the runtime
source and proved otherwise**. The checker described below encodes exactly this
bias: its `NONCOLLECTING` set is the only place a call is declared safe, and
every entry names the runtime line that proves it.

## Why this class of bug is so expensive

Every violation presents the same way and none of it points at the code that is
wrong:

- the symptom is `TypeError: value is not a function`, or a SIGSEGV, **cycles
  later and somewhere else** — wherever the stale pointer is finally
  dereferenced;
- **no runtime GC probe can see it.** At the moment of the collection there is
  nothing for the collector to find, so a from-space scan, a verify-roots pass
  and a rate-1 seeded run all come back clean. `PERRY_GC_VERIFY_EVACUATION` checks that
  reachable slots were forwarded; it cannot check a register it does not know
  exists;
- it is **visible by default only where a poll is emitted.** The back-edge poll
  is on by default again, but `emit_gc_loop_safepoint` emits it only into loops
  `loop_purity::loop_may_allocate` says can allocate — so a default run covers
  this class exactly when execution reaches such a loop, and covers nothing in
  a program whose hot path is a proven alloc-free loop. `PERRY_GC_MOVING_LOOP_POLLS=0`
  removes the coverage entirely.

Four instances shipped in a single day. The detection lag, not the fix, was the
cost every time.

## The five ways it has actually broken

### 1. Slot index past the frame (#7184)

The root store was emitted, and it looked right. But the slot index fell outside
the frame pushed by `js_shadow_frame_enter`, so `js_shadow_slot_bind`
bounds-checked it and **silently returned**. The value was never rooted; the IR
says it was.

*Tell:* a `js_shadow_slot_bind(i32 N, …)` where `N >= the frame size`. There is
no diagnostic — the bind is a no-op by design, because a bounds-check that
panicked would be worse.

### 2. Root store after a collecting call (#7192)

The store was in-frame and correct, but emitted **after** a call that allocates.
Between the allocation and the store, the value lived only in a register.

```llvm
%obj = call ptr @js_object_alloc(i32 4)
%ret = call double @js_call_function(double %a)   ; can evacuate %obj
store ptr %obj, ptr %slot                          ; stores the OLD address
call void @js_shadow_slot_bind(i32 0, ptr %slot)   ; roots a dangling pointer
```

*Tell:* the resulting slot is *rooted* and *dangling* at the same time, which is
why it survives every "is it rooted?" check.

### 3. Method receiver across the argument list (#7206)

The receiver was loaded out of its root, then the argument expressions were
lowered — each of which can allocate — and only then was the call emitted with
the receiver still in the register loaded before the arguments.

*Tell:* a `load` from a root slot, followed by any lowering of a sub-expression,
followed by a use of the loaded register. **Re-read the root after every
collection point** instead of caching the load.

### 4. Computed-read base across the key expression (#7206)

`base[key]` — the base was materialized, then the *key* expression was lowered
(allocating a string, say), then the element read used the stale base.

*Tell:* two operands where one is evaluated first and used last.

### 5. Runtime-cache class (#7226, #7239)

A thread-local or static cell holding a GC pointer that no registered scanner
rewrites. Unlike the register-class bugs above (which go bad intermittently when a
collection lands in a narrow window), an unregistered runtime cache becomes
stale at the first moving collection after it is populated and stays stale
until rewritten.

Real instances: `js_value_typeof` interned its eight result strings in
thread-local `Cell<*mut StringHeader>`s with no registered scanner (#7226);
`json/raw_json.rs`'s cached `"rawJSON"` key (#7226); and the ten runtime caches
in #7239 — `CACHED_ENV`, `CACHED_PERMISSION`, `CACHED_REPORT`, `ERROR_CONSTRUCTOR_PTR`,
`INPUT_HANDLER`, `RESIZE_CALLBACK`, `FRAME_CALLBACKS`, `CURRENT_NEW_TARGET`,
`ACCESSOR_RECEIVER_OVERRIDE`, and `PENDING_FETCH_SIGNAL`.

*Tell:* a thread-local or static cell holding a GC pointer. A test that fails 10/10,
not intermittently, suggests this class rather than a stale register.

**`scripts/gc_root_dominance_check.py` is structurally blind to this class** — it
reads emitted LLVM IR and cannot see a runtime table. The instruments that catch it
are `PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`
on a real workload. When adding a cache of a heap pointer, register it in
`gc_register_mutable_root_scanner` in `gc/mod.rs` in the same commit.

## How to check your work

### 1. The static checker — run this one

**Scope: emitted-LLVM rooting hazards only** — a stale register or an unrooted
alloca in generated code. Within that scope it is the only instrument that sees
a defect before it crashes, which is why it runs first.

**It is blind to three classes, all found the hard way. A clean report is not
evidence for any of them:**

- **Runtime tables and interning caches** (#7231) — it reads emitted IR and
  cannot see a runtime cell. Tell: fails 10/10 rather than intermittently.
- **Unrooted locals in runtime Rust** (#7249) — same reason. It read
  `0 violations` on both sides of a real bug whose fix was a one-line
  `GcSuppressScope` in the `globalThis` bootstrap.
- **Anything its symbol sets do not name** (#7284) — `POLL_CAPABLE_RUNTIME` is
  an *exact emitted-symbol* set. It carried `js_object_get_field_by_name`, which
  codegen never emits, next to `js_object_set_field_by_name`, which it emits
  verbatim. Property sets classified `MOVING: YES`, property gets `MOVING: no`,
  and 31 stale uses were dropped by `--moving-only`. **Audit these sets against
  what codegen actually emits, the way #7227 audits `ALLOC_RE`.**

For the classes above, the instruments that catch them are the schedule/quarantine
arms below and a *dependency-scale* workload — #7280 records 25 curated corpus
files passing while 20 lines of stock zod fail.

**A fourth class was on this list until #7663 and is now covered: the lowering
that actually ships.** The corpus used to be compiled under `PERRY_RS4GC=0` —
the shadow stack — because the checker anchored on `@js_shadow_slot_bind` and
the native lowering emits zero of them. That made a green `gc-root-dominance` a
statement about a lowering that has not been the default on any walkable-frame
target since #7370. **Run both**, and know which one you ran:

```bash
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static

# SHADOW (PERRY_RS4GC=0): still the lowering on arm64_32 watchOS and ARM64
# Windows. Anchors on root stores.
./scripts/gc_root_dominance_corpus.sh ir-corpus
python3 scripts/gc_root_dominance_check.py ir-corpus --moving-only \
  --allowlist scripts/gc_root_dominance_allowlist.json -v

# NATIVE (PERRY_RS4GC=1): the default everywhere else. Anchors on
# `gc.statepoint` `"gc-live"` bundles. Needs an LLVM `opt` -- codegen emits
# `ptr addrspace(1)` root allocas and LLVM inserts the safepoints later, so the
# corpus is `--trace llvm` output plus the production statepoint rewrite.
./scripts/gc_root_dominance_corpus.sh ir-corpus-native --lowering native
python3 scripts/gc_root_dominance_check.py ir-corpus-native --statepoints \
  --moving-only -v
```

Each mode **refuses the other's corpus** rather than reporting it clean: the
native corpus has zero root stores, so `--min-binds` fails there, and the
shadow corpus has zero safepoints, so `--min-statepoints` fails there.

### What `--statepoints` checks, and how it differs

Under native roots a value is a root at a safepoint iff it appears in that
`gc.statepoint`'s `"gc-live"` bundle, and its identity below the safepoint is
the `gc.relocate` result. So "the root store must dominate every later
collection point" becomes:

> No register naming a GC object may be USED below a safepoint unless it is the
> relocated value.

The line that does the work is **tracked vs untracked**. LLVM relocates
`ptr addrspace(1)` SSA values and rewrites their dominated uses, so those are
never stale. Everything else is invisible to it — and Perry NaN-boxes, so a
JSValue spends most of its life as a `double`. Two verdict classes, because
they have two different fixes:

| class | means | fix |
|---|---|---|
| `unrooted` | no `ptr addrspace(1)` value in the register's cast chain is in the safepoint's live bundle. Nothing marks or rewrites the object. | root it |
| `stale` | the object IS in the bundle and is relocated, but a raw copy of its pre-move address is used below | re-derive from the relocated value (`OperandProtection::Reload`) |

Two things this mode gets for free that the shadow modes cannot:
**`NONCOLLECTING` is not consulted** — LLVM already decided which calls are
safepoints and put the answer in the IR, so a wrong entry in that hand-kept
list cannot hide a hazard here; and every safepoint **names its wrapped
callee**, so `--moving-only` classifies against the real symbol rather than
`llvm.experimental.gc.statepoint.p0`.

It parses the emitted LLVM IR, builds per-function CFGs, computes real
Cooper/Harvey/Kennedy dominance, and reports every **collection point** that can
run between the instruction producing a GC value and the root store that
publishes it — that is, a collection point the value's root store does **not**
dominate, which is exactly the rule at the top of this page. Dominance is what
makes the report sound in both directions: the producing instruction must
dominate the bind, so the register being rooted really is the one that
instruction produced on every path. It is one-sided: an unrecognised call counts
as collecting, so a gap in its model costs a false positive, never a missed bug.

For a single file you are iterating on:

```bash
PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_INLINE_SHADOW_SLOT=0 \
  ./target/release/perry compile mycase.ts -o /tmp/mycase --trace llvm
python3 scripts/gc_root_dominance_check.py .perry-trace/llvm -v
```

Both env knobs matter, for different reasons.

`PERRY_GC_MOVING_LOOP_POLLS=1` is what puts `js_gc_loop_safepoint` in the IR. It
is the only collection point a **back edge itself** introduces — a loop whose
body calls nothing that collects still collects, once per iteration, and only
with this on. So a bug that needs a collection between two points of an
otherwise inert loop body cannot appear in the corpus without it.

It is **not** what makes the `MOVING` classification work, and it is not the
only collection point that can run inside a loop — a `POLL_CAPABLE_RUNTIME`
helper called from a loop body is in-loop too. `movers`
(`gc_root_dominance_check.py`, the `movers` property on `Violation`, `StaleUse`
and `UnrootedAlloca`) counts `js_gc_loop_safepoint`, anything in
`poll_reaching`, **and** anything in `POLL_CAPABLE_RUNTIME` — the runtime
helpers that can re-enter JS, such as `js_object_set_field_by_name`,
`js_object_get_field_ic_miss` and `js_closure_call1`. Those are moving with no
poll anywhere near them.

So: turn the knob on, because it widens what the corpus can express, but do not
read a poll-free function as safe.

### `POLL_CAPABLE_RUNTIME` is by EXACT emitted symbol, and that has bitten twice

`movers` is a set-membership test on the callee name, so an entry that names a
symbol codegen does not emit classifies nothing, forever, and looks exactly
like coverage while doing it. Two rounds of this have now been measured:

1. **A real symbol codegen never emits.** The set carried
   `js_object_get_field_by_name` next to `js_object_set_field_by_name`, which
   reads as symmetric coverage of property access. It is not: codegen emits the
   SET verbatim but lowers every GET to `js_object_get_field_by_name_f64`,
   `js_object_get_field_ic_miss` or
   `js_typed_feedback_object_get_field_by_name_f64`, none of which were in the
   set. Property sets classified `MOVING: YES`, property gets classified
   `MOVING: no`, and 31 `--stale-registers` hits on the gate corpus were dropped
   by `--moving-only` as a result — including the shape that faults
   deterministically under `PERRY_GC_PROTECT_FROMSPACE=1
   PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` at zod's `clone`. The protector and the
   checker disagreed; the checker was wrong.
2. **Ten names that were not symbols at all.** `js_apply_function`,
   `js_array_for_each`, `js_array_sort`, `js_call_closure`, `js_call_value`,
   `js_function_call`, `js_invoke_closure`, `js_object_get_property`,
   `js_object_set_property`, `js_string_replace` — extrapolated spellings, none
   of them an `extern "C" fn` anywhere in the runtime. Four of the ten were four
   different ways of saying "call a JS closure", so the single most obviously
   poll-capable operation in the language was covered zero times; the real
   entry points are `js_closure_callN`, which `RECEIVER_SINKS` in the same file
   already spelled correctly.

3. **A real symbol that was simply not in the set** (#7616 / #7453). The two
   rounds above are both a NAME WITH NO REFERENT, and both audits look only in
   that direction. `new URL(input, base)` held a raw `*mut StringHeader` from
   `js_url_coerce_string` across the lowering of `base`; #7453's fix added
   `url_coerce_string` to `ALLOC_RE` — its comment says *"that gap is why the
   checker did not flag #7453"* — and stopped one list short, so the shape was
   never catchable under the mode CI runs. Re-planting that exact code and
   running every mode (#7616):

   | mode | clean | sabotaged |
   |---|--:|--:|
   | `--moving-only` (dominance) | 0 | **0** |
   | `--unrooted-allocas --moving-only` | 0 | **0** |
   | `--stale-registers --moving-only` | 2 | **2** |
   | `--statepoints --moving-only` (the lowering that ships) | 2 | **2** |
   | `--stale-registers` (unfiltered) | 24 | 35 |
   | `--statepoints` (unfiltered) | 15 | 21 |

   Every gated arm blind, both unfiltered arms not — *including* `--statepoints`,
   added in #7663 precisely because the other three were blind to the shipping
   lowering. Adding the one name takes the sabotaged arms to 13 and 8 and leaves
   the clean arms at 2 and 2.

4. **A poll-capable symbol NO audit can ask for** (#8809). `--audit-poll-reach`
   walks only symbols `ALLOC_RE` matches, so a helper that allocates but spells
   it neither `_alloc` nor `_new` nor `_create` is outside its domain entirely.
   `js_private_brand_add` is one: it reaches `js_object_set_field_by_name` in
   three lines, and its own body says *"the marker-key allocation can evacuate
   both the receiver and any live value"* — it opens a `RuntimeHandleScope` for
   exactly that reason. Unlisted, the window `new C()` opens around it
   classified `MOVING: no`, and every `--moving-only` arm dropped a real stale
   instance handle. Round 3's instrument is structurally blind here, which is
   the standing residual: **when you add a runtime helper that can re-enter JS
   or allocate through a path that can, add it to `POLL_CAPABLE_RUNTIME` in the
   same commit.** Nothing will ask you to.

`--audit-poll-capable` is the gate for rounds 1–2 and `--audit-poll-reach` is
the gate for round 3; round 4 has no gate. `gc-root-dominance.yml` runs both
alongside `--audit-alloc-re` before the build.

**Those pre-build audits are also the job's single point of failure, and it has
already cost ten days.** `--audit-poll-reach` went red on `main` on 2026-08-15
over three unlisted symbols and stayed red; because it runs *before* the
compiler build, not one of the four gated arms below it executed until #8809.
Two rooting regressions landed inside that window, and the opt-in PR arm did
not see either (neither PR carried `run-extended-tests`). A red audit is not a
warning about the checker's bookkeeping — it is the whole gate off. `--audit-poll-capable` fails on any entry
that names no exported `extern "C" fn js_*`. When it goes red, **replace** the
phantom with the symbol codegen actually emits rather than deleting it —
deleting turns the audit green and leaves the hole.

`--audit-poll-reach` fails when a symbol `ALLOC_RE` matches reaches a
`POLL_CAPABLE_RUNTIME` symbol through the runtime's own call graph without being
listed itself. It is deliberately NOT "every poll-capable symbol must be
listed" — 297 exported symbols call one directly, and deciding that is a
coverage change with its own hit count. It asserts only that **the checker's two
lists must not disagree about the same symbol**: if ALLOC_RE says a call's
result is a heap value to track, and the runtime shows that call invoking
something this set already grants can re-enter JS, the premise for listing it is
one the set already granted. The reach relation is a fixpoint over exported
symbols (a one-hop version reported 52 names, and re-running after adding them
found 10 more), and comments and string literals are stripped first so a name
mentioned in prose cannot become a premise.

Checking a *plausible* name is not enough. Confirm against emitted IR:

```bash
grep -ho 'call [^@]*@js_[A-Za-z0-9_.$]*(' ir-corpus/*.ll \
  | sed -E 's/.*@([A-Za-z0-9_.$]+)\($/\1/' | sort | uniq -c | sort -rn
```

`PERRY_INLINE_SHADOW_SLOT=0` makes every root store the `js_shadow_slot_bind`
call form the checker anchors on.

`--stale-registers` (#7206) additionally catches values that are *never* rooted
— read out of a root and held in a register across a collection point. That is
the mode that found cases 3 and 4. It ships and works, but **the gate command
above does not pass it**: the bind-anchored scan is the arm that is baselined by
the allowlist, so cases 3 and 4 only surface when you run this mode by hand.

`--unrooted-allocas` (#7207) covers the remaining shape, and is the one the
bind-anchored check is structurally blind to: the value lives in a plain
`alloca_entry` for its whole lifetime, so there is no `js_shadow_slot_bind` to
anchor on and a scan that starts from binds calls the function clean. It found
`lower_call/new.rs`'s inline-ctor `this_slot` independently of any runtime
probe.

**The gate runs this mode as of #7236, and the corpus reads 0.** It could not
before: #7210 measured 66 hits and triaged every one as a false positive, #7235
split the heap-source predicate by movability (98 → 2 on a grown corpus), and
the 2 residuals were one bug — `collectors/pointer_locals.rs` classified
`Type::Symbol` as an immediate, so a `Symbol` local got no shadow slot at all.
Run it by hand when you touch an `alloca_entry` site:

```bash
python3 scripts/gc_root_dominance_check.py .perry-trace/llvm \
  --unrooted-allocas --moving-only -v
```

### 2. The runtime instruments — second, and mind the depth

From #7196:

- `PERRY_GC_SCHEDULE_RATE=1` (with a seed) — collect at every candidate
  safepoint, allocation-paced (#7728): one candidate per
  `PERRY_GC_SCHEDULE_ALLOC_KB` (default 4) of new nursery material. Slow,
  thorough. Add `PERRY_GC_SCHEDULE_ALLOC_KB=0` for the literal every-poll mode
  when the window you are hunting executes only once — it is far slower.
- `PERRY_GC_PROTECT_FROMSPACE=1` — `mprotect` from-space after evacuation so a
  stale read faults immediately instead of reading plausible garbage.
- `PERRY_GC_FROMSPACE_SCAN_ABORT` — now actually runs.
- `PERRY_GC_SCHEDULE_SEED=<u64>` (+ `PERRY_GC_SCHEDULE_RATE`, default `0.05`) —
  collect on a deterministic pseudo-random schedule. At `RATE=1` it collects at
  every safepoint: slow, thorough, maximum pressure. Drop the rate when that is
  *too* blunt — on a workload whose timing it distorts enough to kill somewhere
  uninteresting — and the schedule thins out without losing the property that
  matters. The schedule is a pure function of `(seed, per-thread safepoint
  ordinal)`, so a seed that fails is a reproducer, which is what turns "1 run in
  60" into something you can bisect against.
  `scripts/gc_schedule_fuzz.sh <binary> [seed-count]` sweeps seeds and prints a
  reproduce command per failure.

> **A rate is not a substitute for a schedule.** Re-running one binary 60 times
> re-runs one schedule 60 times; with zero failures in `N` runs the 95% upper
> bound on the true rate is only ~`3/N`, so 120 clean runs bound a 1.7% bug at
> 2.5% — no evidence at all. Varying *when* collections fire is the only cheap
> way to explore the space the bug actually lives in.

> **`PERRY_GC_PROTECT_FROMSPACE_DEPTH` defaults to 4, and that default produces
> FALSE GREENS.** Four levels of retained from-space is not enough to still be
> holding the block your stale pointer is in by the time it is dereferenced.
> **Use 800.** A clean run at the default depth means nothing.

Depth is a **detection-window** knob, not a sensitivity knob, and the
difference matters when the two instruments disagree. A page-set enters the
quarantine only because an evacuating minor actually retired it as from-space
(`arena/quarantine.rs`, and the knob gates only
`copying_reset_from_spaces_and_flip`), so *any* fault on a quarantined address
is a genuine stale read no matter how deep the ring is. Raising the depth
removes false NEGATIVES — evicting a set hands its blocks back to Eden, where
the same read silently succeeds — and cannot manufacture a false positive. So
"the protector faulted, but only at DEPTH=800" is never grounds to doubt the
protector; when it disagrees with the static checker, look at the checker first.
That is how the zod `clone` disagreement was settled.

And when a fault does fire, **walk UP the stack**. The reporter names the frame
that DEREFERENCED the stale value, which is usually not the frame that owns the
register — the value commonly arrives as an argument from a caller that let it
go stale.

And remember the ceiling on all of these: if the collection happens while the
only copy is in a register, there is nothing at that moment for any runtime
probe to notice. These instruments catch the *consequence*, later. The static
checker catches the *cause*, now.

## The mirror image: a missing write barrier (#8185)

Everything above is about a value the collector cannot **find**. The write
barrier is about an edge the collector is never **told about**. The two are
duals, and — this is the part that keeps catching people — **their detectors
are swapped**.

| | rooting bug | missing/deleted write barrier |
|---|---|---|
| what goes wrong | a live value is invisible to the root scan | an old→young edge is absent from the remembered set |
| when it goes wrong | at the collection, silently | not at the store at all; at some *later* minor |
| the detector | the runtime instruments (`PERRY_GC_SCHEDULE_RATE`, `PERRY_GC_PROTECT_FROMSPACE`), plus the static checker for the IR-visible half | a **static IR assertion**, and nothing else |
| the blind spot | the static checker cannot see a runtime-side cache of a heap pointer (see §5) | **every runtime probe we have** |

### Why no runtime probe can see it

A dropped barrier corrupts nothing at the moment of the store. The store still
writes the right bits into the right slot; the object graph is correct. All
that happens is that a remembered-set entry goes unwritten, so the set is
merely *incomplete*. Turning that into an observable failure needs a
conjunction the program has to supply on its own:

1. the parent has to survive into old-gen (or be tenured) **before** the store;
2. the child has to still be in the nursery **at the next minor**;
3. a **minor** collection — not a full mark-sweep, which retraces everything
   and papers over the whole class — has to land in that window; and
4. that edge has to be the *only* path to the child, or some other root finds
   it anyway and the collection is clean.

Miss any one and the minor collects correctly, the program prints the right
answer, and the probe reports success. Nothing was tried.

The individual knobs are worse than merely insensitive — three of them are
aimed at a different property entirely, and it is easy to read their green as
evidence:

- `PERRY_GC_FORCE_EVACUATE` / `PERRY_GC_VERIFY_EVACUATION` verify
  **rewriting**: that every live slot pointing at a forwarded object was
  updated. A slot the collector never traced is not a slot it failed to
  rewrite. Remembering and rewriting are different properties, and the verifier
  only asks about the second one.
- `PERRY_GEN_GC=0` reverts to full mark-sweep, which **does not consult the
  remembered set at all**. It does not make the bug visible; it makes the bug
  unreachable. A green run here is the strongest-looking and emptiest evidence
  of the three.
- `PERRY_GC_SCHEDULE_RATE=1` + `PERRY_GC_PROTECT_FROMSPACE` catch a *stale read
  of a moved object* — condition (4)'s aftermath, on the rooting side of the
  duality. They fault on a dangling from-space pointer, not on a live object
  that was never traced.

  And check the knob you are about to cite is a knob. `scripts/check_gc_env_knobs.py`
  is in `lint` precisely because this drifts: a matrix arm naming a variable
  nothing parses runs the DEFAULT configuration and reports success — hazard 4
  again, one level up. Every name in this document is one the gate has
  confirmed a live parser owns.

This is CLAUDE.md's hazard 4 ("the gate runs but its subject never did") with
the subject inverted: **the absence of a barrier cannot be observed by running
the program.** There is no execution in which "the barrier did not run" is a
distinguishable event.

**Recorded, because it is the whole reason the IR assertions exist (#8183):** a
**release** build with the write barrier deleted from the dynamic-key write
IC's reference arm passes the entire adversarial matrix — old→young edge
fixtures, `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`,
`PERRY_GEN_GC=0`, and forced collection with from-space protection at depth 400
— **byte-identical output, exit 0**, on both a gap fixture and a larger
adversarial one. The static IR test was the only thing that said no.

### What a PR that adds or moves a store on a GC slot owes

Its barrier evidence is a **static IR assertion**, not a behavioural test. Four
things it has to pin, each because a sabotage that skipped it was *not* caught:

1. **The bookkeeping is present** in the pointer-capable arm — the write
   barrier, the layout note, and the string addref. Any one of the three going
   missing is the #5094 / #7511 family of silent stranding.
2. **The arm is REACHED.** Assert a `br i1` *into* the block, not merely that a
   block with that label exists. #8183's third sabotage — routing reference
   values back to the outlined helper — left the arm behind as **dead IR** that
   every content assertion happily inspected, and initially passed. Presence of
   code is not proof it runs.
3. **The negative arm stays clean.** The pointer-free arm must *not* contain
   the bookkeeping. A barrier leaking into it means the discriminator stopped
   discriminating, and the "optimization" is measuring nothing.
4. **Sabotage runs, reported.** Delete each element in turn and record that the
   test goes red. A test that has never failed is a test whose failure mode is
   unknown.

And put it where it runs. `test.yml`'s per-PR `cargo-test` arm is `--lib
--bins` only; `crates/*/tests/*.rs` is nightly/tag (`e2e-scoped` runs only the
suites the diff happens to name). A barrier assertion parked in `tests/`
gates its own PR and no future one — which is exactly the PR that will move the
store. **In-crate `#[cfg(test)]` under `src/`, per #5960.**
`crates/perry-codegen/src/expr/class_field_barrier_tests.rs`,
`index_set_barrier_tests.rs` and `write_pic_barrier_tests.rs` are the shape.

### The `GC_STORE_AUDIT` marker, and what it does and does not prove

Every raw GC-relevant store site carries a nearby marker naming its verdict:

```rust
// GC_STORE_AUDIT(BARRIERED): the slot write is unconditional; the barrier
// below is guarded only by a live test that the stored bits carry no heap
// pointer, which is the barrier's own first test.
```

The classes are `BARRIERED`, `EXTERNAL_BARRIERED`, `ROOT`, `INIT`,
`POINTER_FREE`, `STACK`. `scripts/gc_store_site_inventory.py` (in `lint`) scans
the first-party store sites and fails when one has no marker — so a **new**
store site cannot land with the question unanswered.

Since #8185 landed its second half, the script verifies the **claim**, not
just the comment, for the two classes where a false claim strands objects:

- **`BARRIERED` in `perry-codegen`** is bound to an IR witness. Every call to
  the stem-taking barrier emitters (`emit_write_barrier_slot_generation_tested`,
  `…_value_and_generation_tested`, `emit_jsvalue_slot_store_pointer_tested`)
  must pass a string-literal stem, and the census in
  `crates/perry-codegen/src/expr/barrier_stem_census_tests.rs`
  (`VERIFIED_BARRIER_STEMS`) must list exactly that stem set — the lint script
  fails on drift in either direction, on a stem it cannot resolve to a
  literal, and on a `BARRIERED` marker in any codegen file not bound to a
  census stem. The census test itself (a `--lib` test, so per-PR) compiles a
  probe per stem and, for **every instance** of the stem's gate in the emitted
  IR, asserts a `cond_br` into `<stem>.barrier.<n>`, the
  `js_write_barrier_slot` call inside that block, and the branch predicate
  walked by def-chain back to the `GC_FLAG_TENURED` load and the
  incremental-count load — so `br i1 true` with the dead predicate left in
  place fails, and so does a barrier bypassed in one specialized clone but
  intact in another. Four IR-surgery sabotages (delete the call, hard-wire the
  gate, move the call out of its block, bypass the gate) run in the suite
  against every stem.
- **`BARRIERED` / `EXTERNAL_BARRIERED` in `perry-runtime` / `perry-stdlib`**
  are rustc-compiled, so there is no perry-emitted IR; the claim is verified
  against source structure instead. From the marker to the end of its
  enclosing function there must be a call to a barrier primitive (defined
  under `crates/perry-runtime/src/gc/`) or to a registered discharge helper
  (`RUNTIME_DISCHARGE_HELPERS` in the script), and every registered helper is
  itself re-verified each run to reach a primitive through the call graph —
  deleting the barrier *inside* `note_array_slot` turns every marker leaning
  on it red. Granularity is the enclosing function (two barriered stores and
  one barrier call in the same function still pass), and the script prints
  that limit.

What is still trusted: `ROOT`, `INIT`, `POINTER_FREE` and `STACK` verdicts are
human-audited only, and the script says so in its summary on every run —
`UNVERIFIED (human-audited only, by class): …`. A codegen caller that passes
`write_barrier_needed: false` where `true` was meant is a parameterization bug
neither layer catches. If the verifier's own inputs rot — the census file
missing, the registry parsing to zero entries, a scan matching fewer sites
than its floor — the script exits **2** rather than reading as a clean empty
pass (the `gc_rekeyed_key_tables.py` discipline), and its `--self-test` plants
fifteen shapes, each of which must be adjudicated.

## The corpus problem, and the two corpora (#7280)

**A hand-written corpus cannot express this class of bug at dependency scale,
and for a while nobody could tell, because it was green.**

`scripts/gc_root_dominance_corpus.sh` compiles ~124 `test-files/` sources
chosen for the lowerings they exercise. It reads **zero** in both modes the CI
gate runs — and it read zero while twenty lines of stock `zod` faulted
deterministically under `PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`. #7280 puts it in
one sentence: *25 curated files pass while 20 lines of stock zod fail.*

That is not a size problem. Both corpora were measured on the same compiler with
`--stale-registers --moving-only`:

| corpus | stale uses | what dominates |
|---|---|---|
| curated, 124 sources / 144 modules | 116 | property-GET helper windows, `js_number_coerce`, `js_closure_callN` |
| dependency-scale, 81 modules / 62 MB | 370 | `js_object_assign_one` (object spread) 137, `js_new_function_construct` 102, `js_closure_call*` 30 |

The curated corpus produces 12 of the first population and 1 of the second. A
hand-written test allocates a couple of objects and calls a couple of helpers; a
library spreads objects into objects, boxes every mutable capture because its
closures outlive their frames, and builds values field by field out of data.
**The rooting hazards live in the shapes**, so a corpus without the shapes
cannot express them however many files it has.

So there is a second corpus, generated from a real npm dependency rather than
from anything written for the occasion:

```bash
npm ci --ignore-scripts                       # zod is a package.json devDependency
./scripts/gc_root_dominance_dep_corpus.sh ir-corpus-dep
python3 scripts/gc_root_dominance_check.py ir-corpus-dep --moving-only -v
```

`test-files/gc-dep-corpus/main.ts` is the only entry point; the rest of that
directory reaches the compiler by being imported from it, and the generator
**asserts that every `.ts` in the directory produced a module** — which is a
check a size floor could not be, since ~90 modules of `zod` swamp any count a
missing 40-line source would cross.

Nothing is sampled away: all 81 modules and all 62 MB are checked. Emitting
costs ~8s and the two gated arms ~4s, because those are linear in instruction
count. The `--stale-registers` budget is the expensive one (~5 min): its scan is
superlinear, and 62 MB is 62 MB.

## The CI gate

`.github/workflows/gc-root-dominance.yml` runs the checker on every PR over both
corpora. The whole job is a few minutes plus the compiler build; the two gated
checks are about three seconds each over ~2000 and ~12900 functions.

It is built to be able to fail, against all four hazards in CLAUDE.md:

- the checker's exit status is the job's — no `continue-on-error`, no pipe;
- `concurrency` cancels pull-request runs only, so `main` runs are never starved;
- `--min-files` / `--min-binds` / `--min-funcs` refuse a clean verdict over a
  corpus too thin to have exercised anything, and the run prints
  `checked N functions / M modules` so a silently-empty run is visible;
- `--self-test` proves it still fires on planted fixtures, and
  `--seeded-violations 40` splices collection points into the **real** corpus IR
  and requires all 40 to be reported — that is the arm that catches the checker
  silently losing the ability to read perry's output;
- `--audit-alloc-re` and `--audit-poll-capable` refuse a name that matches no
  exported runtime symbol, in the two tables that decide *whether a register has
  a heap-value source* and *whether the window around it is moving*. Both run
  before the build, because both are static and instant and both have shipped
  dead entries: nine in `ALLOC_RE` across two rounds, ten in
  `POLL_CAPABLE_RUNTIME`.

### The allowlist, and why it is not a number

Known-remaining violations live in `scripts/gc_root_dominance_allowlist.json`,
one entry each with a fingerprint, an issue, and a written justification.

A numeric threshold cannot tell a new violation from an old one: fix one bug,
introduce another, and the total is unchanged and the gate stays green. Worse,
under deadline the cheapest way to green a red build is to raise the number by
one, and nothing in the diff says what was conceded.

So the checker enforces three properties:

1. **an entry that matches nothing fails the build.** When you fix the bug,
   delete the entry in the same PR. That is the ratchet, and it is why a fixed
   bug cannot leave a tombstone that quietly widens coverage later.
2. **an entry suppresses at most its `count`.** A second violation of the same
   shape in the same function is new, and fails.
3. **a violation with no entry fails**, regardless of how many entries exist.

Adding an entry is a code-review event. Bumping a `count` to green a build is
the exact thing this file exists to prevent.

### Promoting this gate

**As of this writing the job is NOT in branch protection's required contexts**,
which means it cannot turn a merge red — hazard 2.

Both of the conditions #7198 named are now met:

- the bind-anchored dominance check is green on `main` with an **empty**
  allowlist (the #7211 entries were deleted when that predicate was fixed in
  #7226);
- `--unrooted-allocas --moving-only` reads **0** and is a step in the job
  (#7236). That was the outstanding one: it was 98 before #7235, 2 after, and 0
  once `Type::Symbol` stopped being classified as an immediate.

So the remaining step is for a **repo admin** to add `gc-root-dominance` to
branch protection's required contexts:

```
Settings → Branches → main → Require status checks to pass
  → add:  gc-root-dominance
```

A workflow cannot do this to itself, and neither can a PR. Until it is done,
this is documentation. Per CLAUDE.md's corollary, promote it **after** the job's
first green run on `main` with the `--unrooted-allocas` step included — a gate
that has never been green in its current shape blocks every open PR the day it
becomes required.

**`gc-root-dominance-statepoints` is a SEPARATE context and a separate
decision.** It was added by #7663 as a second job for exactly that reason: it
reads a different corpus, its floors are about safepoints rather than root
stores, and it should be promotable without dragging the shadow arms along.
Promote it on the same terms — after its first green run on `main`, never
before — and note that its `--max-unrooted` budget is a *ratchet under triage*,
not a calibrated zero: the residual is enumerated by shape in #7664. Lower it as
the population is fixed; a promotion that freezes the budget where it is has
bought a number, not an invariant.

## Rules of thumb

- **Root before you call, not after.** If a value must survive a call, its root
  store belongs above the call, unconditionally. Do not predicate it on a
  cleverness about which callees collect — #7211's `ClassExprFresh` tried that and
  only asked about author-supplied initializers, never about the lowering's own
  emitted `js_object_set_field_by_name` calls.
- **Re-read the root after every collection point.** Never cache a load out of a
  root slot across a call. `rooted_handle_get` exists for this.
- **Evaluate-then-allocate is the hazard.** Any lowering with two or more
  operands where one is materialized before another is lowered needs the first
  one rooted.
- **`--trace llvm` and read it.** Three seconds of the checker beats a day of
  bisecting a `not a function` five cycles downstream.
- **When in doubt, root it.** A redundant shadow slot costs a store. A missing
  one costs a day, and it costs it to whoever hits the crash, not to you.
