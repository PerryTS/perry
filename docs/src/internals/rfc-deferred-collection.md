# RFC: deferred collection

**Status:** proposed, design only. Nothing in this page is implemented beyond
what [the current state](#1-where-a-collection-can-begin-today) describes; it
is written for the owner's review before any code. Lever name in the GC-cost
census: **L2b**.
**Problem:** relocation fan-out under RS4GC statepoints (#8583), and the
hand-kept safepoint allowlist that has already been wrong twice (#11522,
#11523).
**Related:** #11500 (L0), [the GC rooting invariant](gc-rooting-invariant.md),
[the collector](garbage-collector.md), [step bounds](gc-step-bounds.md),
[RFC: rooting by construction](rfc-rooting-by-construction.md).

Every number on this page is tagged **MEASURED** (from the census described in
[the appendix](#appendix-the-census-and-how-to-reproduce-it)) or
**ESTIMATED** (argued, not run). Line numbers are against `main` at
`24a5262bd`; the census itself ran at `a90cd9d44`.

## The idea in one paragraph

Today any call that can *allocate* is a potential collection point, so under
native roots every such call is a `gc.statepoint`, and RS4GC emits one
`gc.relocate` per GC value live across it. This RFC makes three changes.
Allocation never starts a precise collection: when the nursery is full it
takes another block and arms the existing poll word. Precise collections begin
only at **polls**: loop back-edges, a small set of function entries, explicit
runtime polls, `gc()`, and the event-loop and microtask boundaries. A call is
then a safepoint only if its callee **may reach a poll**. In practice that
means the callee can re-enter JavaScript, since every JavaScript body that
allocates in a loop or recursion carries a poll. A helper that merely
allocates stops being a safepoint. So does a helper that reaches JavaScript
only by throwing (the **throw cut**). Inline-cache helpers are split into a
leaf fast path and an out-of-line safepoint slow path.

Non-goal, by owner decision: a conservative or mostly-copying collector as the
primary mechanism. The conservative native-stack scan survives only as the
emergency valve it already is ([§3](#3-the-memory-bound)).

## The evidence (claude-code bundle census)

MEASURED on the claude-code 2.1.112 bundle (78,005 GC-carrying functions).
Liveness is exact SSA liveness over the RS4GC input (after
`always-inline,mem2reg,sccp`), validated against the `gc.relocate` count that
`rewrite-statepoints-for-gc` actually produces.

| quantity | value |
|---|---:|
| safepoints today | 2,113,000 |
| relocations today | 13.89 M |
| what the #8583 estimator predicts for the same bundle | 1.34 × 10⁹ (≈ 100× over) |

**What each lever removes** (cumulative, relative to today, MEASURED):

| lever | safepoints | relocations | Δ relocations |
|---|---:|---:|---:|
| today | 2.113 M | 13.89 M | — |
| L0: existing leaf allowlist also on `invoke` (#11500) | | | −12 % |
| L2: call-graph "cannot reach a collector" | 1.408 M | 11.42 M | −18 % |
| **L2b: "cannot reach a poll"** | 1.314 M | 10.02 M | −28 % |
| **L2b + throw cut** | 1.172 M | 7.39 M | **−47 %** |
| + dropping call-result temporaries (L1) | 1.172 M | 6.37 M | −54 % at most |

L1 is not worth building. RS4GC liveness already ignores dead temporaries, and
the residue is 7 points.

**What today's safepoints are** (MEASURED, 2.113 M sites):

| class | sites | what it means |
|---|---:|---|
| can neither collect nor re-enter JS | ≈ 705 k (33 %) | pure overhead today; removed by L2 |
| only allocate | 94 k | removed by L2b |
| re-enter JS only by throwing | 142 k | removed by the throw cut |
| re-enter JS | 1.17 M | remain safepoints (86 k are internal JS calls, 45 k are indirect) |

The throw-only class covers 145 k call sites: not-callable 57 k, trusted box
read (`js_box_get_bits_trusted`) 24 k, `js_array_from_values` 16 k,
property-read-on-undefined 10.7 k, `js_array_alloc` 8.6 k, object-coercible
7.7 k, this-before-super 5.2 k.

**Live values at the safepoints that remain** under L2b + throw cut (MEASURED):
the mean is 6.3. By count: 109 k have 0, 705 k have 1–4, 283 k have 5–16,
65 k have 17–64, 9.6 k have 65–256, 377 have 257–1024, and 26 have more than
1024.

**The worst function.** `__25747` is the bundle's largest by relocations:

| | today | L2b | L2b + throw cut |
|---|---:|---:|---:|
| safepoints | 7,127 | 6,016 | 3,672 |
| relocations | 3.31 M | 2.67 M | 0.87 M |
| post-RS4GC instructions | 3.66 M | 3.01 M | 1.21 M |
| `opt` wall (RS4GC pipeline) | 125 s | 130 s | 41 s |
| `opt` peak RSS | 2.44 GB | 1.92 GB | 0.75 GB |

L2b alone buys nothing in time on this function, even though it removes 19 %
of the relocations. The throw cut halves the safepoints. So compile time is not
linear in relocations, and the levers are not interchangeable. 90 % of what
remains in `__25747` is values backed by globals: string-literal handles and
class-key arrays. Rematerializing those is a separate change, already in
progress, and it composes with this one.

**#8583 spill decisions.** Five functions exceed the 32 M spill threshold
today, and none does under L2b + throw cut. The estimator's error is the real
problem. `__87158` is estimated at 134.5 M relocations, and 42,444 were
measured. That is 3,000× over, so the function is moved to a shadow frame for
nothing.

**Fast/slow split candidates** among the 1.17 M re-entering sites (MEASURED):

| candidate | sites |
|---|---:|
| property-get IC | 191 k |
| class-field get/set IC | 74 k |
| packed index get/put miss | 60 k |
| class allocation | 31 k |
| template coercion | 20 k |
| `js_box_get_bits` | 163 k |

Together that is about 46 % of the re-entering sites. `js_box_get_bits` is
re-entrant only through a `PERRY_DEBUG` `eprintln!` and the TDZ throw.

## 1. Where a collection can begin today

The nursery half of this idea **already ships**, which changes how the RFC
reads. Nursery pressure has been deferred to polls since `PERRY_GC_MOVING_LOOP_POLLS`
became default-on (`policy.rs:886-944`; runtime and codegen predicates pinned
equal). What this RFC adds is:

- making "precise collections begin only at polls" an **invariant** instead of
  a policy with exceptions;
- the codegen predicate that exploits the invariant.

Every place a collection can start:

| # | entry | file:line | roots | moves? |
|---|---|---|---|---|
| A1 | arena block full → `gc_check_trigger` | `arena/block.rs:939` (from `arena_cell_alloc`, `:920`; reached from generated code by `js_inline_arena_slow_alloc`, `arena/inline.rs:70`) | see A-arms below | — |
| A2 | every `gc_malloc` → `gc_check_trigger_inlined` | `gc/malloc.rs:271` | see A-arms | — |
| A3 | explicit mid-parse checks in JSON | `json/parse_api.rs:299,407,657,836,944`, `json/parse_inline_object.rs:219`, `json_tape/record_materialize.rs:35` | see A-arms | — |
| A-old | `gc_check_trigger` OldReclaim arm: a **full mark-sweep at the allocation point** | `gc/policy.rs:3240-3262` | forced conservative scan (`OldReclaimAllocPoint`) | no |
| A-nur | nursery triggers (ArenaBytes, YoungScavengeCap, MallocCount): **deferred**, `set_safepoint_pending(true)` | `gc/policy.rs:3356-3378` | — | — |
| A-valve | deferral slack (64 MiB since deferral, budget-scaled; `GC_MOVING_DEFER_SLACK_BYTES`, `:1532`) exhausted → direct minor at the allocation point | `gc/policy.rs:3434-3436` | forced conservative scan (`NurseryChurnSlackValve`) | no (#7682) |
| A-assist | otherwise, if a budgeted cycle is active: a mutator-assist step at the allocation point | `gc/policy.rs:3468-3476` | **precise**: budgeted cycles skip the conservative scan by design (`gc/cycle.rs:853-863`) | no |
| A-emerg | block reservation or `alloc` failed → emergency full | `gc/mod.rs:876` | forced conservative scan (`EmergencyReclaim`) | no |
| D | root-lock exit flushes a deferred request: `exit_gc_root_lock` → `flush_deferred_gc_request` runs `gc_check_trigger`, a direct minor, or a full | `gc/roots.rs:170-183`, `gc/policy.rs:1614-1642` | as the arm it runs | per arm |
| P-loop | loop back-edge poll `js_gc_loop_safepoint` → `gc_safepoint_moving_minor` | `gc/policy.rs:4045`, `:3792`; emitted at `stmt/loops.rs:7901`/`:8030` | precise, **declared** | **yes** |
| P-pump | outermost microtask-pump boundary | `promise/microtasks.rs:393-406` | precise, declared | yes |
| P-host | runtime budgeted step: event-loop pump, regex quanta | `lib.rs:803`, `regex/perex_runtime.rs:59` → `gc_runtime_safepoint_poll` (`gc/policy.rs:4850`) | precise | no (budgeted) |
| E | explicit `gc()` / `perry/gc` `minor()` / `js_gc_memory_pressure` / idle reclaim and compaction | `gc/policy.rs:4932`, `:5072`; `gc/pressure.rs:60`; `gc/idle_reclaim.rs:550` | precise (except `minor()`) | yes |
| T | test-only named collection points | `gc/collection_points.rs` | — | — |

Four facts make every allocating call a statepoint today, and the RFC has to
remove all four:

1. **A-assist reads precise frame roots at an allocation point.** A budgeted
   cycle's `RootScan` (`gc/cycle.rs:921`) and its atomic `FinalRootRemark`
   (`gc/cycle.rs:1134-1165`) can both run inside a mutator assist. That
   collection does not move anything, but it has to *enumerate* the roots of
   every frame on the stack.
2. **The native-root walker skips an unmapped frame silently.** Its lookup is
   `index.match_records(return_address)` (`gc/roots/stack_maps.rs:1824`): a
   frame whose return address has no record contributes no roots, and nothing
   reports it. So a frame suspended at a `gc-leaf-function` call whose callee
   does collect loses its roots, and nobody is told. That is the failure mode
   behind #11522 and #11523.
3. **The codegen default is "collecting".** `classify_direct_callee`
   (`gc_call_effects.rs:35`) is a hand-audited allowlist with `Unknown` as the
   fallback. `transitive_leaf_functions` (`:534`) extends it only to
   module-local functions.
4. **`AllocNoReentry` is research-only.** It becomes a leaf only under
   `PERRY_GC_SAFEPOINT_ONLY` (`codegen/helpers.rs:276`; runtime contract at
   `gc/policy.rs:1409-1490`), and the knob-ledger in
   `.github/workflows/gc-native-roots.yml:80` says no CI arm has ever set it.

The arms A-old, A-valve and A-emerg are sound *today* without statepoints at
the allocation call, because they force the conservative scan and do not move
anything. A-assist, D and every leaf misclassification are not.

## 2. Target semantics

### The invariant

> **D1.** A collection phase that reads frame roots or relocates anything
> begins only at a *declared poll*. Declared polls are exactly:
> `js_gc_loop_safepoint`, the function-entry polls below, `runtime_poll()`
> call sites inside the runtime, the outermost microtask boundary, the
> event-loop pump, and explicit collection requests (`gc()`,
> `perry/gc`, memory-pressure callbacks, idle reclaim).
>
> **D2.** An allocation may take a new block, arm the poll word
> (`set_safepoint_pending`), run *heap-only* budgeted work (mark propagation,
> weak processing, sweep, reclaim), or, only when the valve fires, run a
> conservative non-moving collection. It never starts a root-reading or moving
> phase.
>
> **D3.** A call site is a safepoint iff its callee *may reach a declared
> poll*. The throw cut refines this: reaching a poll only through the unwind
> of a throw does not count ([§4.6](#46-exceptions-and-unwinding)).

The runtime changes needed for D1/D2, all in `gc/policy.rs` and `gc/cycle.rs`:

- **A-old becomes a deferral.** It sets the pending flag with the same slack
  valve A-nur uses. The existing precise OldReclaim arm in
  `gc_safepoint_moving_minor` (`:3855-3875`) already runs the identical full at
  the poll. #5476's guarantee changes from "one `gc_check_trigger` completes
  the reclaim" to "the next poll completes it", and that needs owner sign-off
  ([open question 1](#8-open-questions-for-the-owner)).
- **A-assist stops before root-reading subphases.** `RootScan` and
  `FinalRootRemark` become "requires a declared safepoint": an assist that
  reaches one returns and arms the poll. The heap-only phases may still run in
  an assist, because they read no frame. Soundness is the incremental-update
  argument the collector already makes (mark barrier, allocate-black, final
  remark). Once the remark runs at a poll, no register can hold a white object
  that the remark did not see. ESTIMATED sound; the step-bounds suite
  (`gc/tests/step_bounds.rs`) and the weak-read-barrier tests are the gate.
- **D stops collecting.** A root-lock exit merges the request into the pending
  flag instead of running it. This is option 2 of #11523, and it closes that
  bug as a class instead of auditing helpers one at a time.
- **The contract stops being a knob.** `SafepointOnlyContract` (`:1409`) turns
  into the default invariant. Under `cfg(test)` and the `gcaudit` profile a
  precise collection that begins undeclared panics, like today's `strict`. In
  release it is structurally unreachable, because no undeclared path calls a
  precise collector any more. The `Heal` arm is deleted under the knob
  kill-policy: once D1 holds, nothing can produce it.

### Where polls go

Loop back-edges already carry polls, gated by `loop_may_allocate`
(`loop_purity.rs:75`) and armed by one volatile load
(`stmt/loops.rs:8030-8075`). They bound allocation in *iteration*. Two more
kinds of repetition allocate without passing a back-edge:

1. **Recursion.** Consider `function t(d){ return d ? {l: t(d-1), r: t(d-1)} : null }`.
   It has no loop, and the module fixed point (`transitive_leaf_functions`)
   deliberately admits pure recursive SCCs as leaves. Without a poll it would
   allocate 2^d objects with no collection. Rule: **one entry poll per
   recursive SCC of the direct call graph**, placed in the SCC member with the
   most incoming edges from inside the SCC. Calls into that SCC then become
   safepoints, which is correct, because the SCC can collect.
2. **Indirect invocation.** A closure, a method, a callback or a generator
   resume body. Every indirect call is already a safepoint (unknown callee), so
   the poll costs no relocations *at the call site*. It belongs in an
   **indirect-entry prologue**: the address stored in `ClosureHeader.func_ptr`
   and in the method tables points at a stub that runs the armed-load poll and
   falls into the body. Direct calls keep using the poll-free entry, so the
   module fixed point still proves them leaf.

**Why not a poll at every function entry.** The census ran a mode with the
internal-call closure disabled, which is what all-entry polls amount to:
12.18 M relocations against 7.39 M (MEASURED). Entry polls everywhere would
give back almost everything L2b buys. **Return polls** are rejected: a callee's
return is followed by the caller's next poll, and a return poll turns every
call to that function into a safepoint for nothing.

Two coverage gaps have to be closed before this ships:

- The doc comment at `stmt/loops.rs:7893-7899` says the specialized `for`
  lowerings, `for-of` and `for-in` do not emit the poll. That needs
  re-verifying against the tree.
- A poll-coverage checker over emitted IR: every back-edge of a loop that may
  allocate, and every recursive SCC, has a poll.

### Computing "may reach a poll"

The predicate has two halves, both computed as fixed points.

**Generated code** (per module, in codegen). `transitive_leaf_functions` gains
the three classes below instead of one bit. Seeds: an indirect call, a call to
a runtime symbol classified `Reenters`, or a declared poll.

**The runtime** (over the *linked archives*). This is a static
reference graph built from object-file relocations, the same construction the
census used (`callgraph.py`, appendix):

- **Nodes** are `(object, section)`; with `-ffunction-sections` that is one
  node per function, and ICF aliases resolve through the symbol table.
- **Call edges** are `call`/`jmp` relocations.
- **Address-taken edges** are `lea`/`mov` and data-section relocations
  (vtables, fn tables). These do *not* propagate on their own, because the
  indirect call that uses the address is itself a seed.
- **Seeds:**
  - a declared poll entry;
  - an indirect call inside Perry code, minus an audited, reasoned exemption
    list (`core::fmt`, lazy-init cells, drop glue, arena internals: 27
    patterns in the census);
  - an unresolved external outside a libc/libm allowlist.
- **Cuts:** the allocation-trigger path (`gc_check_trigger`, collector entry
  points). Under D2 these no longer collect.
- **Throw cut:** also cut `js_throw` and the fatal-report tail
  (`exception::print_uncaught`).

The result classifies every exported `js_*` symbol:

| class | codegen treatment | census count (exported C symbols, of 5,708) |
|---|---|---:|
| `Leaf`: reaches no collector entry at all | `gc-leaf-function` on `call` and `invoke` | 2,610 (L2) |
| `AllocOnly`: reaches a collector only through the cut allocation trigger | leaf, **only after D1/D2 ship** | +273 (L2b 2,883) |
| `ThrowOnly`: reaches a poll only through the throw cut | leaf, only after D1/D2 ship | +274 (3,157) |
| `Reenters` | statepoint | 2,551 |

**Source of truth.** I recommend a **generated, committed table**,
`crates/perry-codegen/src/gc_effects.generated.tsv`, which
`classify_direct_callee` reads through `include_str!`. The checker
regenerates it in CI and **fails hard** on drift in the unsafe direction (a
symbol the table calls `Leaf`/`AllocOnly`/`ThrowOnly` now reaches a seed). It
reports drift in the safe direction (a new leaf) as "regenerate". The
hand-written arms in `gc_call_effects.rs` shrink to the entries the checker
cannot see: inline-asm barriers, `llvm.*`, and symbols codegen defines itself.
Annotations in source (`#[gc_effect(leaf)]`) are the alternative. They read
better in review, but an annotation is a claim and the graph is the proof, so
the checker is needed either way. Annotations would only add a second list to
drift (#7510 is what that looks like).

**Soundness against the #11522 class.** Four layers, in the order they catch
things:

1. **The CI checker.** A symbol whose committed class is weaker than
   `Reenters` but whose graph reaches `js_proxy_get`, `js_closure_call*`,
   `js_native_call_*`, a coercion entry, or a poll entry fails `lint`, and the
   failure prints the shortest witness path (`why.py`). A sabotage test plants
   a `js_proxy_get` call in a leaf helper and asserts red.
2. **Per target.** Inlining and `cfg` arms differ between targets. The checker
   runs on the Linux x86-64 archives and the macOS aarch64 archives at
   minimum, and the table is the *intersection* of leaves. Whether Windows
   (`cargo xwin`) is also required is
   [open question 4](#8-open-questions-for-the-owner).
3. **An unmapped-frame verifier at runtime.** At every precise collection
   under `cfg(test)`, `gcaudit` or `PERRY_GC_VERIFY_FRAMES=1`, a frame whose
   return address falls inside a Perry-generated function but matches no
   stack-map record is a panic. That frame is exactly a leaf-marked call that
   led to a collection. The index already answers function containment
   (`stack_maps.rs:1818-1823`). Functions that carry *no* record are not in
   that table, so codegen emits a range table for every generated function.
   Paired with `PERRY_GC_SCHEDULE_SEED` at `RATE=1`, this checks the
   classification dynamically on every test that runs.
4. **The root-dominance checker.** It stays as it is. Its `--statepoints` mode
   trusts the IR's placement, which is exactly why layers 1 to 3 have to exist.

## 3. The memory bound

Between two polls, allocation is not bounded by anything the program does. It
is bounded only by the valve. Two regimes reach that:

- **Straight-line code:** module init; string-pool chunks with ~80 k strings;
  the bundle's giant entry chunks.
- **A single heavy runtime call:** `JSON.parse`, `Array.from`, string building.

The design:

1. **The nursery grows by blocks while a poll is pending.** This is today's
   A-nur behaviour, unchanged.
2. **The valve is the only allocation-point collection left (A-valve,
   A-emerg).** It is sound without statepoints at the allocation site because:
   - it scans the whole native stack and the `setjmp`-captured callee-saved
     registers, including Rust runtime frames;
   - it resolves interior pointers (`gc/trace.rs:552`);
   - it is **non-moving**: the forced scan makes the copying minor ineligible,
     so no unmapped frame needs a rewrite;
   - it is per-thread, like the heap.

   What it cannot see is a GC reference held in a form that is neither a
   plausible address nor NaN-boxed, such as a scaled index or a pointer
   outside its object. That is the same exposure the valve has today; D2 does
   not widen it. The cost is known: a conservative cycle retains ambiguously
   and runs no copying minor. Firing on every cycle measured +364 % to
   +5371 % `heap_used_bytes` on the ratchet probes (`gc/scan_fallback.rs:12-14`).
   So the valve must stay exceptional, and the `scan_fallback` census turns
   that into a gate ([§7](#7-validation-plan)).
3. **Heavy runtime calls fall into three kinds:**
   - *Output-dominated* (`JSON.parse` result, `Array.from`, `join`, `repeat`):
     what they allocate is their result, which is live, so a collection partway
     through reclaims only garbage that existed at entry and was collectable at
     the previous poll. No poll is needed; the overshoot is at most the garbage
     at entry. `JSON.parse` already suppresses GC across tape materialization
     (`parse_api.rs:299-302`).
   - *Garbage-producing* (regex replace over large input, repeated
     intermediate buffers): these get **`runtime_poll()` every N MiB** (propose
     N = the young scavenge cap), with their state held in `RuntimeHandleScope`.
     The call graph then classifies them `Reenters` automatically. That is
     correct, and it is what regex quanta already do (`perex_runtime.rs:59`).
     The alternative per helper is malloc-backed scratch, which is outside the
     GC heap and needs no poll.
   - *JS-calling* (`sort` with a comparator, `forEach`, iterators): the
     callback's indirect-entry poll covers them.
4. **Straight-line code** is bounded by *allocation sites × per-site size*,
   which is finite but not small. ESTIMATED: unknown for the bundle's init
   chunks, so it has to be measured first (a new `bytes_since_last_poll`
   high-water mark in the `[gc]` exit summary). If the measurement says it is
   needed, emit a **statement-boundary poll** every K allocating top-level
   statements in bodies above a site threshold, following the precedent of
   `STRAIGHT_LINE_STORE_OUTLINE_MIN_SITES` (`codegen/helpers.rs`). At a
   top-level statement boundary few values are live, and #8583-spilled bodies
   use shadow frames anyway. This step is gated on the measurement, not
   proposed blind.
5. **Hard emergency:** unchanged. `gc_try_emergency_reclaim` (`gc/mod.rs:876`)
   runs when the OS refuses a block, and the valve slack scales with the heap
   budget (`gc/heap_budget.rs`).

**Against the owner's rule ("minimize RSS, never trade compute").** Compute:
collection *work* does not change, and the wins are compile time plus fewer
statepoint spills on hot paths. RSS: the nursery half of the deferral already
ships, so the new RSS exposure is two things:

- OldReclaim moving from the allocation point to the next poll. ESTIMATED at
  most one growth quantum, 32 MiB (`GC_OLD_GEN_RECLAIM_GROWTH_BYTES`), in
  compute-only code, and zero in event-loop code, which reaches a poll first.
- A budgeted cycle waiting for a poll to remark.

Both are measured before step S3 flips, with acceptance criteria in
[§7](#7-validation-plan). The design adds no new RSS-for-compute trade.

## 4. Interactions

### 4.1 Write barriers and incremental marking

Barriers are untouched: every barrier helper is already `CannotCollect`. Under
D2 the budgeted stepper runs heap-only phases anywhere and root phases only at
polls, so its initial mark and its final remark move to polls. Pause
accounting (`[gc-step-bounds]`) is unchanged, because the remark is already
atomic ([step bounds](gc-step-bounds.md#final-root-remark-is-atomic-on-purpose)).
What does change is *latency*: a cycle whose marking finishes between polls
waits for the next one.

### 4.2 Generational promotion

Copying minors ran only at polls already (P-loop, P-pump, E). The nursery can
overshoot its scavenge cap by the allocation between polls. A copying minor's
cost is O(survivors), so overshoot costs RSS, not pause. A larger effective
nursery also lowers premature promotion. The promoted-cohort full
(`policy.rs:3936`) is already poll-only.

### 4.3 Threads

Each `perry/thread` agent has its own arena, pending flag and polls, and the
poll word is a process-global superset (`gc/poll_arm.rs`). An agent armed by
another thread's deferral pays one spurious out-of-line call, which is today's
behaviour. `SerializedValue` deep copies mean no cross-heap pointers exist.
`GC_UNSAFE_ZONES` blocks polls and the valve alike, so growth during an unsafe
zone is unbounded, exactly as today. That is out of scope, but it is noted in
the open questions.

### 4.4 Event loop, microtasks, async and generators

The pump boundaries are declared polls. `await` returns to the pump. A
generator or async resume is an indirect call into the state-machine body, so
the body's indirect-entry prologue polls. Async-transformed locals live in heap
cells, not frames. Nothing new is required.

### 4.5 Native and FFI callbacks

A call into an external symbol that is not on the libc allowlist is a seed, so
the generated frame below it is at a statepoint. Native frames in between
carry no records and the walker already skips them. The JS callback entered
from native code starts at an indirect-entry poll. Native-held GC values stay
the business of napi handle scopes and registered scanners, which is
unchanged.

### 4.6 Exceptions and unwinding

- **A `Reenters` callee invoked with `invoke`** gets relocations on both edges,
  as today. RS4GC relocates into the landing pad, and
  `retype_landing_pads_for_statepoints` (`function/precise_roots.rs:561`)
  stays.
- **A `ThrowOnly` callee** reaches JS only by unwinding. Between the throw and
  the landing pad no poll can run: the throw helper's allocation of the error
  cannot collect (D2); `js_throw` (`exception.rs:320`) restores savepoints and
  transfers without running JS; the `setjmp` transport is a `longjmp`. So the
  values live into the landing pad are the pre-call values, valid without
  relocation, and the call can carry `gc-leaf-function` on both `call` and
  `invoke`.
- **Two paths had to be checked:**
  - The *fatal* throw (`try_depth == 0`) runs `exit` listeners (JS) with the
    stack intact (`exception.rs:378-388`). This is sound only because none of
    those frames ever resumes: the process exits. The cut list names
    `print_uncaught` for exactly this reason.
  - `Error.prepareStackTrace` is evaluated lazily when `.stack` is read, not at
    construction (`error.rs:1280-1296`), so building an error runs no JS.
- **The residue.** `js_box_get_bits` is `ThrowOnly` once its `PERRY_DEBUG`
  `eprintln!` is made JS-free. That is 163 k sites.

### 4.7 Shadow-frame targets

On `arm64_32` watchOS, ARM64 Windows and #8583-spilled functions, roots are
runtime-maintained slots and relocation is a reload. The same class table
feeds the shadow lowering's notion of a collecting call. Fewer collecting calls
means fewer post-call reloads (`OperandProtection::Reload`) and fewer root
stores forced to dominate them. The valve is sound there too, by the same
conservative argument. ESTIMATED: runtime win only, because these targets pay
no RS4GC compile cost.

### 4.8 The root-dominance checker and the instrument knobs

- **The checker.** `gc_root_dominance_check.py` keeps `NONCOLLECTING` and
  `POLL_CAPABLE_RUNTIME` by hand today (`:459`, `:1477`), and its
  `poll_reaching` (`:1215`) is an IR-level version of D3. Both hand lists
  become *derived* from the generated table, with the existing containment
  test (`gc_call_effects.rs:602-660`) generalized from the box/closure family
  to the whole table. Under D1 the `--moving-only` classification becomes the
  *only* classification: a non-moving collection at a non-poll is by
  construction the conservative valve.
- **`PERRY_GC_SCHEDULE_SEED` / `RATE`.** These select among *handled
  safepoints*: polls and the pump (`gc/schedule.rs:288`). Under D1 those are
  exactly the points where a collection may begin, so the seeded schedule
  becomes *complete*: at `RATE=1` it collects at every legal collection point.
  It still cannot fire at an allocation, by design, and it no longer needs to.
- **`PERRY_GC_FORCE_EVACUATE`, `PERRY_GC_PROTECT_FROMSPACE`,
  `PERRY_GC_VERIFY_EVACUATION`:** unchanged, and they compose with the
  unmapped-frame verifier.
- **`PERRY_GC_SAFEPOINT_ONLY`** is deleted: its property becomes the default,
  and its research arm has no CI coverage.

## 5. Fast/slow splits for IC helpers

Pattern (precedents already in the tree: `js_inherited_read_cache_hit_f64`,
`js_lazy_array_index_probe`, `js_transition_ic_spill_append`, all
`CannotCollect` probes that answer `TAG_HOLE` on a miss):

```llvm
%v = call i64 @js_object_get_field_ic_fast(i64 %obj, ptr %site) "gc-leaf-function"
%miss = icmp eq i64 %v, TAG_HOLE
br i1 %miss, label %slow, label %join, !prof !cold
slow:                                        ; the only statepoint
  %s = call i64 @js_object_get_field_ic_slow(i64 %obj, ptr %key, ptr %site)
  br label %join
join:
  %r = phi i64 [ %v, %entry ], [ %s, %slow ]
```

- **Contract of `_fast`.** It is a `Leaf` or `AllocOnly` symbol in the
  generated table: it serves a data-property hit and returns `TAG_HOLE` for a
  getter, proxy, dictionary miss or anything else it cannot serve. The
  classification comes from the checker, not from a comment.
- **Relocation stays confined to the cold block.** RS4GC places
  `gc.relocate`s only at the slow statepoint. Its relocation-via-alloca
  rewrite, followed by `mem2reg`, produces phis at `%join` that merge each
  live value with its relocated copy. The hot path keeps its values in
  registers, with no statepoint spill or reload. Codegen can additionally
  outline the slow block to a per-function cold stub so the spills do not
  inflate the hot function's frame.
- **What splits do *not* change.** The slow call is still a statepoint with
  the full live set, so the **static** relocation count and RS4GC compile time
  hardly move. Splits are a runtime lever (spills on the hot path), not a
  compile-time lever. Their measurement is dynamic: executed statepoints per
  `perf stat` run, plus cc bundle wall time.
- **Order.** Property get first (191 k sites), then class-field get/set, then
  the packed index get/put miss, then class allocation. `js_box_get_bits`
  needs no split. The throw cut plus removing the `eprintln!` makes the whole
  helper `ThrowOnly`.

## 6. Migration plan

Each step lands on its own, carries its own gate, and can be reverted alone.

| step | change | removes (MEASURED unless marked) | gate | risk |
|---|---|---|---|---|
| **S0** | #11500: leaf-mark audited helpers on `invoke` too | −12 % relocations | codegen unit test + sabotage; `gc.statepoint` count on a try-heavy fixture | low: same audited table |
| **S1** | #11522 (`js_array_length` Proxy arm, and the other flagged symbols) and #11523 (root-lock flush) fixed; land the call-graph checker and the census tools in `scripts/`; switch `classify_direct_callee` to the generated **L2** table | −18 % cumulative (L2). L2 is sound under **today's** runtime: a symbol that reaches no collector entry cannot collect | checker in `lint` with sabotage test; unmapped-frame verifier in `gc-stress` | low for code, medium for the checker's own completeness (exemption list) |
| **S2** | replace #8583's `(slots + sites) × sites` with a linear liveness estimator: per function, backward liveness of root allocas across the classified safepoints of the pre-RS4GC IR, O(instructions + roots) | today's 100× overestimate; spurious shadow-frame spills (`__87158`: 134.5 M estimated vs 42 k measured) | estimator vs census rank correlation on the bundle; the #8586 post-RS4GC budget stays as backstop | low: backstop exists |
| **S3** | runtime D1/D2: A-old deferred, assist stops before root phases, root-lock flush sets pending, contract becomes the invariant (heal arm deleted); indirect-entry and recursive-SCC polls; poll-coverage checker | no relocations yet; enables S4/S5 | [§7](#7-validation-plan) RSS and valve gates; `strict` panic under test and `gcaudit` | **medium: RSS (#5476) and remark latency** |
| **S4** | L2b: `AllocOnly` → leaf | −28 % cumulative | same checker; seeded schedule with verifier | low once S3 holds |
| **S5** | throw cut: `ThrowOnly` → leaf; `js_box_get_bits` debug print made JS-free | −47 % cumulative; spill-threshold functions 5 → 0; `__25747` 125 s → 41 s, 2.44 → 0.75 GB | landing-pad fixtures under forced evacuation + verifier | low |
| **S6** | fast/slow splits, property get first | runtime only; ESTIMATED up to ~46 % of re-entering sites get a leaf hot path | dynamic statepoint count; bench suite | low per IC |

**One deviation from the proposed order.** The throw cut cannot precede S3.
The throw-only helpers *allocate* the error (and `js_array_from_values` /
`js_array_alloc` allocate unboundedly). Under today's runtime that allocation
can reach A-assist, a precise root scan at an unmapped frame, which is the
silent-skip hazard. One sub-step can land early: throw helpers whose *only*
allocation is the error object, if they allocate it through a no-collect path.
`arena_alloc_gc_no_collect` (`arena/allocators.rs:78`) is the precedent, but
today it is a try-current-block probe, so this needs a no-collect "take a block"
variant.

## 7. Validation plan

| instrument | what it must show | when |
|---|---|---|
| `gc-stress` (required) under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1` | green, with `copied_objects > 0` asserted (a gate must assert its subject ran) | S1 onward |
| `PERRY_GC_SCHEDULE_SEED=<s> PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0` + `PERRY_GC_PROTECT_FROMSPACE=1` + unmapped-frame verifier | zero verifier panics over the gap suite and the root-dominance corpus; `loop_polls=` and forced-collection counters non-zero | S1, S3, S4, S5 |
| `gc-root-dominance.yml` over `gc_root_dominance_corpus.sh` | allowlist stays empty; hand lists replaced by the generated table with a containment test | S1 |
| call-graph checker + sabotage test | red on a planted `js_proxy_get` in a leaf; table drift reported | S1 onward |
| cc bundle census (appendix) | the numbers in [the evidence](#the-evidence-claude-code-bundle-census) reproduced within ±1 % at each step's landing; a nightly ratchet on relocations and safepoints | every step |
| `scan_fallback` census | `NurseryChurnSlackValve` and `OldReclaimAllocPoint` = 0 on the gc-ratchet probes and the gap suite; `safepoint_drain_count` > 0 | S3 |
| peak RSS on allocation-heavy straight-line fixtures (new: a 100 k-string pool init, a 50 MB object-literal chunk, a recursive tree build without loops) and the cc bundle (startup plus a scripted session) | ≤ 2 % peak-RSS regression against pre-S3 (proposed bar, owner to confirm); the recursive fixture's RSS bounded by its live set, which proves the SCC poll | S3 |
| `[gc-step-bounds]`, `final_remark_max_us` | no pause regression; remark latency distribution reported | S3 |
| compile time and RSS for the bundle build | S5: `__25747` class of functions off the #8583 spill list; total build wall reported | S2, S5 |

## 8. Open questions for the owner

1. **#5476's guarantee.** Accept OldReclaim moving from "this allocation" to
   "the next poll" (ESTIMATED ≤ 32 MiB extra old-gen residency in compute-only
   code)? If not, OldReclaim stays at the allocation point behind the forced
   conservative scan, which is sound under D1 because it is non-moving, and
   S3 keeps only the other three runtime changes.
2. **Poll placement.** Indirect-entry prologue plus recursive-SCC entry polls,
   as proposed, against all-function entry polls. The latter is simpler but
   measured at 12.18 M instead of 7.39 M relocations.
3. **Straight-line polls.** Measure first (proposed) or add statement-boundary
   polls to large init bodies pre-emptively?
4. **The table's authority.** A generated, committed table (proposed) or
   annotations? Which targets must the checker run on: Linux x86-64 plus macOS
   aarch64, or also Windows through `cargo xwin`?
5. **The valve.** Keep 64 MiB of slack? Should a valve firing on the gap suite
   or the ratchet probes be a hard `pr-gate` failure?
6. **Budgeted-cycle latency.** Accept that root-reading phases wait for a poll?
7. **The fatal-throw carve-out.** Exit listeners run on an intact stack whose
   frames never resume. Accept this as part of the throw cut?
8. **The RSS acceptance bar** for S3 (proposed: ≤ 2 % peak RSS on the ratchet
   probes and the bundle).
9. **The census as CI.** A nightly job on the measurement host with ratcheted
   relocation and safepoint totals, or run by hand at each step?
10. **`GC_UNSAFE_ZONES`.** Growth during an unsafe zone stays unbounded. Leave
    it out of scope?

## Appendix: the census and how to reproduce it

The census was run on the measurement host (`perrymaster:/root/gcmeasure`,
checkout `a90cd9d44`, claude-code 2.1.112, LLVM 22). S1 moves the tools into
`scripts/`. The pipeline:

1. The bundle is compiled with `--trace llvm`, and the per-module IR is
   concatenated (2.58 GB of `.ll`).
2. `gcm.cpp` is an `opt` pass plugin with two halves:
   - `gcm-tag` runs *before* `mem2reg`: it records root allocas and tags every
     value stored into one.
   - `gcm-stats` runs after `mem2reg,sccp`: it computes exact SSA liveness of
     every `ptr addrspace(1)` value and counts, per safepoint, the values live
     across it under each leaf set.

   It is validated on the top functions by running the real
   `always-inline,function(mem2reg,sccp),rewrite-statepoints-for-gc` pipeline
   and counting `gc.relocate` (`runR.sh`, `r25747.sh`).
3. `callgraph.py` builds the section-level reference graph over the linked
   runtime and stdlib archives: 66,977 function sections and 272,717 edges.
   Its inputs are `nonjs_indirect.txt` (the audited exemptions) and
   `l2b_opt_cuts.txt` (the throw cut). It emits the leaf sets for L2, L2b and
   L2b + throw cut. `why.py` prints the shortest path from a symbol to a seed.
4. `aggregate.py` produces per-function tables and the totals quoted above
   (`agg.txt`, `stats.tsv`).
