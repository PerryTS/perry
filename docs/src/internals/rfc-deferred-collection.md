# RFC: deferred collection

**Status:** accepted with amendments; migrating. The owner's decisions of
2026-09-27 are recorded in [§8](#8-decisions-2026-09-27) and folded into the
text. Where a decision overrides the original proposal, the text says so.
Nothing here is implemented beyond what
[the current state](#1-where-a-collection-can-begin-today) describes. S0
(#11500) is PR #11531, and S1 and S3 are in progress
([§6](#6-migration-plan)). Lever name in the GC-cost census: **L2b**.
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

**Fast/slow split census** (MEASURED with the same tools, a follow-up run).
In this run, 17 helpers are treated as leaves whose re-entering work happens
only on their slow paths: the property-get IC, the class-field get/set IC, the
box reads, the callee unbox, the packed index get and put-miss, class
allocation, template coercion, and a few more. Fourteen of the 17 are new
leaves relative to the L2b + throw-cut set. The rest were already leaves there
because they are throw-only (`js_box_get_bits_trusted`,
`js_closure_unbox_callee_checked`). The 14:

- the IC helpers: `js_object_get_field_ic`, `js_class_field_get_ic`,
  `js_class_field_set_ic`, `js_packed_arraylike_index_get`,
  `js_typed_feedback_array_index_get_fallback_boxed`,
  `js_put_value_set_packed_miss`;
- `js_box_get_bits`, `js_template_string_coerce_box`,
  `js_ctor_return_override`, `js_array_push_f64`;
- the class-allocation family: `js_object_alloc_class_inline_keys`,
  `js_object_alloc_class_inline_keys_stamped`, `js_build_class_keys_array`,
  `js_gc_typed_shape_id_for_keys`.

Stacking all 17 on L2b + throw cut gives:

| | today | L2b + throw cut | **+ splits** | + splits + L1 |
|---|---:|---:|---:|---:|
| relocations | 13.89 M | 7.39 M | **3.23 M (−77 %)** | 2.88 M |
| safepoints | 2.11 M | 1.17 M | **0.53 M** | 0.53 M |

Live values per remaining safepoint, with splits: 69 k safepoints have 0,
323 k have 1–4, 111 k have 5–16, 25 k have 17–64, 4.3 k have 65–256, 340 have
257–1024, and 20 have more than 1024.

Relocations in the top functions, today → with splits:

| function | today | with splits |
|---|---:|---:|
| `__25747` | 3.31 M | 615 k |
| `GW7` | 380 k | 151 k |
| `__27679` | 408 k | 57 k |
| `m_A` | 108 k | 39 k |
| `__28905` | 107 k | 33 k |
| `__14872` | 72 k | 47 k |
| `__52336` | 80 k | 0 |

The former spill cases `__84092`, `__85198` and `__80686` each fall below 4 k.

**Caveat, and it matters for the plan.** The −77 % is measured with the splits
*stacked on* L2b + throw cut. The splits land first in the revised plan (S2 in
[§6](#6-migration-plan)), where their standalone gain on today's runtime is
smaller and **has not been measured**. The S2 work measures it before claiming
a number, and part of the 17 are leaves only under deferral anyway
([§5](#5-fastslow-splits-for-ic-helpers)).

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
> weak processing, sweep, reclaim), or run a conservative non-moving
> collection in exactly two cases: the valve fires, or the OldReclaim arm
> becomes due (kept at the allocation point by
> [decision 1](#8-decisions-2026-09-27)). It never starts a phase that reads
> frame roots *precisely*, and it never starts a moving phase.
>
> **D3.** A call site is a safepoint iff its callee *may reach a declared
> poll*. The throw cut refines this: reaching a poll only through the unwind
> of a throw does not count ([§4.6](#46-exceptions-and-unwinding)).

The runtime changes needed for D1/D2, all in `gc/policy.rs` and `gc/cycle.rs`:

- **A-old stays at the allocation point.** This is
  [decision 1](#8-decisions-2026-09-27), and it overrides the original
  proposal to defer it. A-old is a forced-conservative, non-moving full, so it
  is compatible with D1, and #5476's "one `gc_check_trigger` completes the
  reclaim" guarantee stands. The existing precise OldReclaim arm in
  `gc_safepoint_moving_minor` (`:3855-3875`) keeps racing it to the poll.
  Moving A-old is revisited only once a measurement shows no RSS cost. Its
  count (`OldReclaimAllocPoint`) is reported in [§7](#7-validation-plan), not
  gated.
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

**Source of truth** ([decision 4](#8-decisions-2026-09-27)): a **generated, committed table**,
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
   runs on three targets: the Linux x86-64 archives, the macOS aarch64
   archives, and Windows through `cargo xwin`
   ([decision 4](#8-decisions-2026-09-27)). The table is the *intersection*
   of the leaves found on those three.
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
2. **The conservative arms are the only allocation-point collections left:**
   A-valve, A-emerg, and A-old, which [decision 1](#8-decisions-2026-09-27)
   keeps. They are sound without statepoints at the allocation site because:
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
   So the valve must stay exceptional. [Decision 5](#8-decisions-2026-09-27)
   keeps the 64 MiB slack and makes a valve firing on the gap suite or the
   ratchet probes a **hard `pr-gate` failure**, backed by a counter that
   proves the check ran ([§7](#7-validation-plan)).
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
   high-water mark in the `[gc]` exit summary).
   [Decision 3](#8-decisions-2026-09-27) makes this a hard rule: if a
   module-init body or large entry chunk exceeds the RSS bar of
   [decision 8](#8-decisions-2026-09-27), codegen adds
   **statement-boundary polls** to *those bodies only*, one every K
   allocating top-level statements. `STRAIGHT_LINE_STORE_OUTLINE_MIN_SITES`
   (`codegen/helpers.rs`) is the precedent for a size-gated body
   transformation. At a top-level statement boundary few values are live, and
   #8583-spilled bodies use shadow frames anyway.
5. **Hard emergency:** unchanged. `gc_try_emergency_reclaim` (`gc/mod.rs:876`)
   runs when the OS refuses a block, and the valve slack scales with the heap
   budget (`gc/heap_budget.rs`).

**Against the owner's rule ("minimize RSS, never trade compute").** Compute:
collection *work* does not change, and the wins are compile time plus fewer
statepoint spills on hot paths. RSS: the nursery half of the deferral already
ships, and OldReclaim stays where it is ([decision 1](#8-decisions-2026-09-27)).
The only new RSS exposure is a budgeted cycle waiting for a poll to remark,
plus whatever straight-line bodies turn out to allocate between polls. Both
are measured before the runtime-invariant step flips. The acceptance bar is
[decision 8](#8-decisions-2026-09-27): at most +2 % peak RSS on any single
probe, no regression of the median across the ratchet corpus and the cc
bundle, and no compute regression.

## 4. Interactions

### 4.1 Write barriers and incremental marking

Barriers are untouched: every barrier helper is already `CannotCollect`. Under
D2 the budgeted stepper runs heap-only phases anywhere and root phases only at
polls, so its initial mark and its final remark move to polls. Pause
accounting (`[gc-step-bounds]`) is unchanged, because the remark is already
atomic ([step bounds](gc-step-bounds.md#final-root-remark-is-atomic-on-purpose)).
What does change is *latency*: a cycle whose marking finishes between polls
waits for the next one. [Decision 6](#8-decisions-2026-09-27) accepts this,
provided pause and remark latency are measured on a server fixture.

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
zone is unbounded, exactly as today. [Decision 10](#8-decisions-2026-09-27)
keeps that out of scope, but adds a diagnostic counter for the bytes
allocated inside unsafe zones.

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
    `print_uncaught` for exactly this reason. [Decision 7](#8-decisions-2026-09-27)
    accepts the carve-out on one condition: a test that throws uncaught into
    an allocation-heavy `exit` listener and runs under forced evacuation and
    the unmapped-frame verifier.
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

### Why a fast path is a sound leaf on *today's* runtime

This argument is why the owner moved splits ahead of the runtime invariant
(to S2). On today's runtime a collection can begin inside a call only through
the entries tabled in [§1](#1-where-a-collection-can-begin-today):

- an allocation reaching `gc_check_trigger` (A1–A3, and through them
  A-assist, the arm that reads precise frame roots);
- a root-lock exit flushing a deferred request (D);
- a JS re-entry reaching a poll, or `gc()` (P, E);
- an indirect call or an unresolved external, whose target is unknown.

A fast entry whose **whole call graph** does none of these reaches no
collector entry. That is exactly the checker's **L2 `Leaf`** class, and L2
needs no runtime invariant, because it is sound against every entry above as
the runtime stands. It excludes three things:

- *Any allocation*, even a small one. Today an allocation can land in
  A-assist, which reads precise frame roots, and a leaf-marked frame would be
  skipped silently (`stack_maps.rs:1824`).
- *Any `GcRootRegistryGuard`*, because of #11523.
- *Any throw.* Throwing makes a helper `ThrowOnly`, which waits for the throw
  cut. A fast entry therefore answers `TAG_HOLE` where the full helper would
  throw: the TDZ read, not-callable, and a derived-constructor primitive
  return.

The source of truth is the S1 checker run over the `_fast` symbol, not the
reading below. The reading below says which of the split census's helpers
have a hit path that qualifies (read at `24a5262bd`):

| helper (sites) | hit path | today? |
|---|---|---|
| `js_object_get_field_ic` (191 k) | `pic_outlined_mru_hit` (`object/field_get_set/ic_miss.rs:1378`): shape-stamp compare plus slot load; feedback counters are `CannotCollect` | **L2 leaf** |
| `js_class_field_get_ic` / `_set_ic` (74 k) | guard (`CannotCollect`) plus slot load, or slot store with `js_object_set_field` plus barrier (`typed_feedback/guards.rs:732-830`) | **L2 leaf** (the set path's barrier and layout note are `CannotCollect`; verify `js_object_set_field`'s tail) |
| `js_box_get_bits` (163 k) | registered box, non-TDZ value: one load (`box.rs:1115-1150`); the TDZ throw and the `PERRY_DEBUG` print go to the slow path | **L2 leaf** |
| `js_box_get_bits_trusted`, `js_closure_unbox_callee_checked` | value or closure-header read; the only other arm throws | **L2 leaf** once the throw arm is on the slow path |
| `js_packed_arraylike_index_get`, `js_typed_feedback_array_index_get_fallback_boxed` (60 k with put-miss) | dense element read, or the `CannotCollect` lazy-array probe (`array/subclass_packed_index.rs:35-80`); the `js_array_get_f64` / `js_dyn_index_get` fallbacks go to the slow path | **L2 leaf** |
| `js_put_value_set_packed_miss` | the existing-key way store (`packed_ways_store`, whose own comment says "nothing here allocates or runs user code") is L2 leaf; the key-add memo `packed_add_try` stores through overflow storage and may allocate | **split**: the way store is a leaf today; the add memo is `AllocOnly` |
| `js_template_string_coerce_box` (20 k) | string input returns itself; number input allocates (`builtins/numbers.rs:774-783`) | **split**: the string arm is a leaf today; the number arm is `AllocOnly` |
| `js_ctor_return_override` | object, undefined or base-primitive: returns a value; a derived primitive throws | **L2 leaf** once the throw arm is on the slow path |
| `js_array_push_f64` | in-capacity push is a store plus length; growth allocates | **split**: the in-capacity push is a leaf if the checker agrees; growth is `AllocOnly` |
| class allocation: `js_object_alloc_class_inline_keys{,_stamped}`, `js_build_class_keys_array`, `js_gc_typed_shape_id_for_keys` (31 k) | allocation *is* the operation | **`AllocOnly`**: a leaf only after L2b deferral (S6) |

Consequence: everything above marked "L2 leaf" can land in S2. Class
allocation, and the allocating arms of put-miss, template coercion and push,
are carried by S2's machinery but switched to leaf only in S6.

### How the split is emitted, and what it does to the static count

- **The `_fast` contract.** Serve the hit, and return `TAG_HOLE` for anything
  else: a getter, a proxy, a dictionary miss, a throw condition. Its class
  comes from the generated table.
- **Relocation stays confined to the cold block.** RS4GC places
  `gc.relocate`s only at the slow statepoint. Its relocation-via-alloca
  rewrite, followed by `mem2reg`, produces phis at `%join` that merge each
  live value with its relocated copy. On the hot path, values stay in
  registers, with no statepoint spill or reload.
- **Read the census number with care.** The split census counted each split
  site as a *leaf call*, which models the hot path. If the slow call stays an
  ordinary statepoint in an inline cold block, that statepoint carries the
  same live set, and the **static** relocation count and RS4GC compile time
  fall far less than −77 %. The win is then mostly at run time.
- **Getting the static number too.** The slow call itself must stop being an
  RS4GC statepoint. The candidate form is a **per-call-site shadow spill**:
  - mark the slow call `gc-leaf-function`;
  - store the frame's live GC values into shadow slots before it, and reload
    them after. This is the #8583 shadow-frame mechanism applied per call site
    instead of per function.

  The walker skips the frame's unmapped PC and finds the values in the shadow
  frame. This needs the pre-RS4GC live set, which is what the linear liveness
  estimator (S4) computes, and it has to satisfy the
  [rooting invariant](gc-rooting-invariant.md)'s dominance rule for the slot
  stores. Until it exists, S2 is measured as a runtime lever. The S2 work
  measures both its static and its dynamic gain on today's runtime before
  claiming either.

## 6. Migration plan

Revised order ([decision](#8-decisions-2026-09-27) of 2026-09-27). Each step
lands on its own, carries its own gate, and can be reverted alone. The
percentages are the census's cumulative figures, and each is valid only for
the configuration it was measured in.

| step | change | removes | gate | risk |
|---|---|---|---|---|
| **S0** | #11500 (PR #11531): leaf-mark audited helpers on `invoke` too | −12 % relocations (MEASURED) | codegen unit test + sabotage; `gc.statepoint` count on a try-heavy fixture | low: same audited table |
| **S1** *(in progress)* | generated call-effects table + call-graph checker; census tools into `scripts/`; **conservative reclassification** of the #11522 helpers (`js_array_length`'s Proxy arm and the other flagged symbols) and the #11523 root-lock-flush helpers to `Unknown` (the flush itself changes in S5); `classify_direct_callee` switched to the generated **L2** table | −18 % cumulative (L2, MEASURED). L2 is sound on **today's** runtime | checker in `lint` on three targets, with a sabotage test; unmapped-frame verifier in `gc-stress` | low for code; medium for the checker's own completeness (the exemption list) |
| **S2** | **fast/slow IC splits** for the helpers marked "L2 leaf" in [§5](#5-fastslow-splits-for-ic-helpers), property get first | the census's −77 % is stacked on L2b + throw cut and counts split sites as leaf calls; the standalone gain on today's runtime is **not yet measured**, and S2 measures it (static and dynamic) first | each `_fast` symbol is L2 `Leaf` in the checker; forced evacuation + seeded schedule + verifier over the IC fixtures; bench suite | low per IC |
| **S3** *(in progress, `perf/remat-global-backed-roots`)* | rematerialize global-backed roots: string-literal handles and class-key arrays are reloaded from their global after a safepoint instead of relocated | 90 % of what remains in `__25747` under L2b + throw cut is global-backed (MEASURED); the whole-bundle figure is measured by S3 | root-dominance corpus; forced evacuation | low |
| **S4** | linear liveness estimator replaces #8583's `(slots + sites) × sites`: per function, backward liveness of root allocas across the classified safepoints of the pre-RS4GC IR, O(instructions + roots) | today's ~100× overestimate; spurious shadow-frame spills (`__87158`: 134.5 M estimated vs 42 k measured) | estimator vs census rank correlation; the #8586 post-RS4GC budget stays as backstop | low: backstop exists |
| **S5** | runtime invariant D1/D2: assist stops before root phases, root-lock flush sets pending, contract becomes the invariant (heal arm deleted), OldReclaim **unchanged** (decision 1); indirect-entry and recursive-SCC polls; poll-coverage checker; straight-line measurement (decision 3) | no relocations by itself; enables S6/S7 | [§7](#7-validation-plan): RSS bar, valve hard gate, remark latency; `strict` panic under test and `gcaudit` | **medium: RSS and remark latency** |
| **S6** | L2b: `AllocOnly` → leaf, including the allocating fast arms of S2 | −28 % cumulative on its own configuration (MEASURED, without splits) | same checker; seeded schedule + verifier | low once S5 holds |
| **S7** | throw cut: `ThrowOnly` → leaf; `js_box_get_bits` debug print made JS-free | −47 % cumulative without splits (MEASURED); spill-threshold functions 5 → 0; `__25747` 125 s → 41 s, 2.44 → 0.75 GB; with splits stacked, 13.89 M → 3.23 M | landing-pad fixtures under forced evacuation + verifier; the fatal-throw test (decision 7) | low |

**Ordering constraints that remain.**

- **The throw cut (S7) must follow the runtime invariant (S5).** The
  throw-only helpers *allocate* the error, and `js_array_from_values` /
  `js_array_alloc` allocate without bound. Today that allocation can reach
  A-assist, a precise root scan at an unmapped frame. S2 avoids this by moving
  every throw arm to the slow path.
- **L2b (S6) must follow S5.** That is D2's whole point.
- **The static half of S2's gain needs S4's liveness.** The per-call-site
  shadow spill in [§5](#5-fastslow-splits-for-ic-helpers) depends on it. S2
  can ship its runtime half without it.

## 7. Validation plan

| instrument | what it must show | when |
|---|---|---|
| `gc-stress` (required) under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1` | green, with `copied_objects > 0` asserted (a gate must assert its subject ran) | S1 onward |
| `PERRY_GC_SCHEDULE_SEED=<s> PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0` + `PERRY_GC_PROTECT_FROMSPACE=1` + unmapped-frame verifier | zero verifier panics over the gap suite, the root-dominance corpus and the IC split fixtures; `loop_polls=` and forced-collection counters non-zero | S1, S2, S5–S7 |
| `gc-root-dominance.yml` over `gc_root_dominance_corpus.sh` | allowlist stays empty; hand lists replaced by the generated table with a containment test | S1 |
| call-graph checker + sabotage test, on Linux x86-64, macOS aarch64 and Windows (`cargo xwin`) | red on a planted `js_proxy_get` in a leaf; unsafe drift fails, safe drift reported | S1 onward |
| census as a **ratchet** (decision 9): relocation and safepoint totals may only go down | a small representative corpus in regular CI; the full cc census nightly through `/root/perry-heavy.sh` | S1 onward |
| valve **hard `pr-gate` failure** (decision 5) | `NurseryChurnSlackValve` = 0 on the gap suite and ratchet probes, with a live counter proving the check ran; `safepoint_drain_count` > 0; `OldReclaimAllocPoint` reported, not gated (decision 1) | S5 |
| peak RSS on allocation-heavy straight-line fixtures (a 100 k-string pool init, a 50 MB object-literal chunk, a recursive tree build with no loops), the ratchet corpus, and the cc bundle (startup plus a scripted session) | decision 8: at most +2 % peak RSS on any single probe, no median regression across the corpus and the bundle, no compute regression. The recursive fixture's RSS is bounded by its live set, which proves the SCC poll. A body over the bar gets statement-boundary polls (decision 3) | S5 |
| server fixture: pause and remark latency (decision 6) | `[gc-step-bounds]` / `final_remark_max_us` distribution reported against pre-S5; no pause regression | S5 |
| fatal-throw test (decision 7) | an uncaught throw with an allocation-heavy `exit` listener, under forced evacuation + verifier, matches node | S7 |
| unsafe-zone growth counter (decision 10) | bytes allocated inside `GC_UNSAFE_ZONES` reported in the `[gc]` exit summary (diagnostic, not gated) | S5 |
| compile time and RSS for the bundle build | functions leave the #8583 spill list; `__25747` class timed; total build wall reported | S2, S4, S7 |

## 8. Decisions (2026-09-27)

All ten open questions of the first draft are **resolved**. The owner's
decisions ([PR comment](https://github.com/PerryTS/perry/pull/11528#issuecomment-5855338615)):

1. **#5476 / OldReclaim:** not moved for now. OldReclaim stays at the
   allocation point. It is non-moving, so it is sound under D1. Revisit only
   once measurement shows no RSS cost.
2. **Poll placement:** as proposed. Indirect-entry prologue polls, one poll per
   recursive SCC, and loop back-edge polls. Measured: 7.39 M vs 12.18 M
   relocations for polls at every function entry.
3. **Straight-line polls:** measure first, with a hard rule. If a module-init
   body or large entry chunk exceeds the RSS bar (item 8), codegen adds
   statement-boundary polls to those bodies only.
4. **Table authority:** a generated, committed table plus the call-graph
   checker in CI, run on Linux x86-64, macOS aarch64, **and** Windows via
   `cargo xwin`.
5. **Valve:** keep 64 MiB slack. A valve firing on the gap suite or the
   ratchet probes is a **hard `pr-gate` failure**, with a counter asserting
   the check actually ran.
6. **Budgeted-cycle latency:** accepted. Measure pause and latency on a server
   fixture.
7. **Fatal-throw carve-out:** accepted, with a test: an uncaught throw plus an
   allocation-heavy exit listener.
8. **RSS bar for S5** (S3 in the first draft's numbering): at most +2 % peak
   RSS on any single probe, AND no regression of the median across the ratchet
   corpus and the cc bundle. Compute must not regress either.
9. **Census as CI:** a ratchet; relocation and safepoint totals may only go
   down. A small representative corpus in regular CI, and the full cc census
   nightly through `/root/perry-heavy.sh`.
10. **`GC_UNSAFE_ZONES`:** out of scope, but add a diagnostic counter for
    growth inside unsafe zones.

**Plan order, decided afterwards:**

- S0 = #11500 (PR #11531).
- S1 = the generated table, the checker, and the conservative reclassification
  for #11522/#11523.
- S2 = fast/slow IC splits, moved up because an IC hit on a plain data
  property neither allocates nor re-enters.
- S3 = rematerializing global-backed roots.
- Then the liveness estimator, the runtime invariant and polls, L2b, and the
  throw cut.

**One new item for the owner, raised by the split census:** the static
relocation figure (−77 %) is realized only if the split's slow call stops
being an RS4GC statepoint, through the per-call-site shadow spill in
[§5](#5-fastslow-splits-for-ic-helpers). Is that form wanted, or should S2
stay a runtime-only lever?

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
