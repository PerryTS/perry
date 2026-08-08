# Perry engine plan — status quo and what is left

**Goal (owner):** best performance, best RSS footprint, minimal binary size.

**Tracker:** #7294 (routing only — this document is authoritative). **History:**
every dated status section, incident narrative and superseded sequencing lives
in [`engine-plan-history.md`](engine-plan-history.md); this file holds only the
current state and the remaining work so it stays readable across context loads.
Last synced **2026-08-08** (v0.5.1350). Since v0.5.1345: the gc-ratchet is
repaired, re-pinned, and liveness-proven (#7609 — fail open per cell, fail
closed on the verdict; owner action: promote to required after its first green
`main` run); the element-shape invariant gained a real revocation matrix
(#7608) and its first consumer — the versioned loop clone, `keep[j].v` at
**node parity**, 3.15× (#7612); and promote-on-first-copy landed (#7613):
`json_pipeline` 500k copies the 268 MB cohort ONCE — wall −24.6% AND peak RSS
−21%, the first change to improve both goal axes at once. #7592 total:
**60.4 s → 3.86 s (~6× bun)**, `JSON.parse` (~742 ms) is the remaining tail.
The Layer-1 emitter migration is **started** (#7615: campaign map, per-module ledger,
1 of 88 modules done). The v0.5.1299 public-baseline sweep is
kept as the baseline measurement event; rows fixed since are annotated in place
rather than overwritten, because they were measured individually rather than in
a fresh sweep.

| Concern | Detail lives in |
|---|---|
| GC rooting correctness | [`src/internals/rfc-rooting-by-construction.md`](src/internals/rfc-rooting-by-construction.md) |
| The rooting invariant + checker blind spots | [`src/internals/gc-rooting-invariant.md`](src/internals/gc-rooting-invariant.md) |
| Representation selection (unbox-by-default) | [`representation-selection-rfc.md`](representation-selection-rfc.md) |
| How each conclusion below was reached | [`engine-plan-history.md`](engine-plan-history.md) |

---

## Status quo

### 2026-08-07/08 in one table — what closed, what opened

Twenty-two merges, v0.5.1324 → v0.5.1345. The construction campaign's measured
levers are now **all worked or retired**, and the day's dominant discovery —
`json_pipeline` at 500k records, found by the first published baseline sweep in
five days — went **97.6× → ~8× bun** in four PRs:

| item | closed by | result |
|---|---|--:|
| survivor-promotion handoff livelock | #7594 | 57.2 s → 12.1 s |
| constant pacing bands (both generations) | #7596 | → 5.8 s; 4 cycles, none futile |
| `charCodeAt` through the dynamic-bitwise helper | #7601 | fnv1a 11.2× |
| statement-position `push` length computation | #7600 | 2.77× pure-push |
| array-push write barrier (parent-side gate) | #7602 | `push_cls` 1.33–1.38× |
| typed-shape install re-derivation | #7586 | `push_cls` 1.091× |
| **Map/Set subclass as raw header (SIGBUS)** | #7573 | memory safety |
| **Array subclass as raw header (SIGSEGV)** | #7603 | memory safety |
| inherited Array statics on a subclass | #7605 | `MyArr.from` works |
| iterator-helpers surface dead (class-id collision) | #7583 | whole TC39 surface |
| second class-id collision (JSX/rawJSON) + scanning gate | #7589 | gate in `lint` |
| `known_failures.json` suppression → ratchet | #7599 | 37 entries → 10 |
| public baseline unpublishable (2 independent causes) | #7593 | `check` exits 0 |

**The `declared type as layout proof` bug class now has a named fix pattern**
(#7573/#7603): brand-check on `GcHeader.obj_type` at the shared runtime
funnels, redirect subclass receivers onto the spec-generic engine, PLUS guards
at any codegen tier that never calls into the runtime — #7603 proved the
runtime funnel alone cannot stop the inline-store tiers. Remaining known member
of the family: none filed. Adjacent leftovers: `ArraySpeciesCreate` on
subclasses, static-method GET form, `instanceof` a subclass (#7575).

**Remaining top levers, in order:** the #7592 remainder (~8× bun: two-hop
promotion copies 268 MB twice — promote-on-first-copy design is on the issue
with the fixed-point trap named; and `JSON.parse` 742 ms); class-field-store
barriers (the half #7602 could not reach); #7480 repsel element-shape proofs;
Layer-1 emitter migration (#7615 — campaign map published, template slice landed,
1 of 88 modules).

**Gate debt still open:** #7554 (gc-ratchet CI has measured nothing since
2026-08-05 — REPAIR THIS BEFORE the next GC-pacing change, which needs it),
#7502–#7507 (root-lowering suites partly vacuous), #7300 (flaky codegen tests),
#7604 (zeal can arm without firing on compute-only benches), #7606 (two macOS
gc-rooting gap crashes, untriaged), #6847 reopened (zlib link on macOS).

### GC correctness — the four layers

*The shape, stated once: a GC-managed pointer exists somewhere the collector
does not know about, across a point where the collector can run. The pointer
has three homes, each needing its own mechanism.*

| Layer | Home | Mechanism | Status |
|---|---|---|---|
| **0** | *enabler* | in-process LLVM | ✅ shipped (#7301), default cargo feature (#7353) |
| **1** | `perry-codegen` lowering code | `Raw`/`Rooted` discipline | design **validated & corrected** (#7459 — the RFC's own constructor was `E0499`); combinator form proven on the real emitter (#7461); the raw-pointer-across-lowering bug shape **eliminated crate-wide** (#7453, #7462–#7465). **Migration started**: campaign map + per-module ledger in **#7615**; `expr/url_main.rs` migrated end to end as the template slice (#7617), which found `URL.canParse`/`URL.parse` still carrying #7453's window. 1 of 88 modules; 262 hazard sites remain. **Measured limit, stated once: on the real emitter this does NOT make the bug fail to compile** — `FnCtx` has no interior mutability, so the borrow form is unbuildable on it; the combinator removes the bug from the path of least resistance and the ledger denies the escape hatch, and that is all |
| **2** | emitted code's liveness | statepoints | ✅ **the default**, target-aware (#7370): native roots where the runtime can walk frames, shadow stack elsewhere |
| **3** | `perry-runtime` hand-written Rust | `RuntimeHandleScope`, non-optional | per-module ceilings (#7457): **595 of 705 modules locked at zero**, 107 listed with ceilings, 999 sites, and the list can only shrink — a cleaned module cannot regress (#7458). `across_*` combinators are the prescribed form (#7455). **End state not reached:** the raw accessor is still reachable inside listed modules |

### Repsel stack (the unbox-by-default campaign)

Phases **1 / 2 / 3a (#6909) / 3b (#6911) / 4a (#6915 + #7421/#7425) / 4b
(#6919)** are all merged; #6904's 26× histogram is closed (#7485 deleted the
dead 4b prototype flag). Next gap:
**element-shape proofs through array reads** — `keep[j].v`, route decided in
**#7480**: both candidate routes share one prerequisite (a per-array
homogeneous-element-shape invariant, construction-maintained, self-healing
like 4a's dense bit), consumed first by the #5093 versioned-loop clone, then
by element `Ptr<Shape>`. Prerequisite and consumer both landed (#7496, #7612);
the consumer's growth-forwarding crash is #7660. **The element `Ptr<Shape>`
half is still open and the kernel re-measures at 34.5× node, not the 6.2×
#7480 recorded** — see backlog item 6 for the current table and for why the
named-class arm's node baseline is not the literal arm's.

### Object construction — the dominant cost (#7469 campaign)

**This is the top row of the backlog and the best-measured part of the engine.**
Symbolicated decomposition of `churn` on the pinned quiet host
(`PERRY_DEBUG_SYMBOLS=1`, 1500 leaf samples, best-of-3):

| variant | Perry | node | ratio |
|---|--:|--:|--:|
| `churn` (full) | 2.72 s | 0.17 s | 16.0× |
| `churn_alloc` — object literal + push | 2.44 s | 0.14 s | 17.4× |
| **`push_cls` — `new Node(v,w)` + push** | **3.99 s** | 0.14 s | **28.5×** |
| `push_num` — numbers into array | 0.30 s | 0.11 s | 2.7× |
| `churn_read` — element reads only | 0.35 s | 0.08 s | 4.3× |

`push_num` at 2.7× shows the array machinery is fine; subtracting it puts
**~79% of `churn` in object construction**. Within construction, **~76% is GC
and feedback bookkeeping and 7.7% is the allocation itself**:

| group | share | ticket |
|---|--:|---|
| `gc::layout` side tables (`layout_forget_*` 14.5%, `layout_note_slot` 7.9%, `js_gc_init_typed_shape_layout` 7.7%, …) | **33.6%** | **#7510** (construction/death half of #5094) |
| `_tlv_get_addr` | 17.0% → 27.0% → **1.1%** | closed by **#7565** (it grew as a *share* while everything round it shrank) |
| write barriers | 16.1% | **#7511** — *correctness-first: a missed barrier is a use-after-free, not a slowdown* |
| typed-feedback guards | 9.2% | repsel 3b |
| array helpers | 6.2% | partly closed by #7501 |
| **the actual allocation** | **7.7%** | — |
| user code | 3.7% | — |

That table is the **v0.5.1299 decomposition and is now superseded** — three of
its rows are closed. Re-profiled on v0.5.1325 (#7578, two independent `sample`
runs of `push_cls`, leaf shares):

| item | then | **now** |
|---|--:|--:|
| **`gc::layout::typed_shape_layout_entry`** | — | **~25%** ← new top lever |
| **write barriers** (4 symbols) | 16.1% | **~25%** (#7511) |
| user code | 3.7% | ~21% |
| `_tlv_get_addr` | 27.0% | **1.0%** ✅ #7565 |
| `gc::layout_tables::layout_forget_object` | 14.5% | **1.7–2.9%** ✅ #7525/#7532 |

**Nothing regressed** — the two survivors grew as *shares* because the rows
around them collapsed. `_tlv_get_addr` and `layout_forget_object` are no longer
levers and should not be worked.

**The new top lever, `typed_shape_layout_entry` (~25%)**, is not the
`ValidateSlots` loop: `push_cls` takes the `js_gc_declare_typed_shape_layout`
path (confirmed in the emitted IR — one call to `declare`, zero to `init`), so
#7515/#7532 are working. It is the install itself, whose hit path
`layout.rs:1022` documents as reducing to "the two header bit-writes
`shape_install_shared` would have performed".

**Partly addressed by #7586** (v0.5.1332): `push_cls` **1.091×**, `churn_alloc`
1.075×, `churn` 1.042×, at **+0 bytes by construction** (no codegen crate is
touched, so emitted IR cannot change). `deeplist` pays 0.7–1.3%, reproducible —
pointer fields put it on the validating entry point. The cost was never the call
overhead: **~30 of the ~70 hit-path instructions re-derived compile-time
constants of the class**, because the FFI boundary makes them opaque — 12
normalising two `(pointer, length)` pairs into slices only ever compared as
integers, ~11 of `words_intersect` setup over two immutable globals, ~6
recomputing the slot kind.

### ⛔ Two remedies that look obvious and are wrong. Do not rebuild them.

An earlier revision of this section proposed inlining the hit path at the `new`
site, reasoning that every argument but the object pointer is a compile-time
constant and that this is the shape #7566 won 1.81× on. **Both halves of that
were tested in #7586 and both are wrong.**

1. **Outlining/inlining the frame is not the lever — it is a regression.** The
   prologue does look like #7566's shape (`sub sp, sp, #0x150`, six `stp` pairs
   — a 336-byte frame and twelve callee-saved spills per construction, sized by
   LLVM for a descriptor build that runs once per shape). Outlining it behind
   `#[cold] #[inline(never)]` cut the frame to 80 bytes and the spills to zero,
   and made things **slower**: `push_cls` 0.72 → 0.75 s, `churn_alloc`
   0.72 → 0.79 s. Those spills are cheap dual-issued stores off the critical
   path, and keeping six arguments live to forward to the outlined call costs
   more in register moves than the prologue saves. **This function is bound by
   instruction count, not frame size.**

2. **⛔ Having codegen OR `GC_OBJ_TYPED_LAYOUT_INTACT` into the inline `new`'s
   header word is a use-after-free factory.** It is seductive because it is
   genuinely free — `declare`-path classes must have an empty pointer mask, so
   since #7566 the inline `new` already writes its `GcHeader` as one i64
   constant, and OR-ing one more bit costs +0 instructions and +0 bytes.

   It breaks the unwritten invariant **"intact ⟹ a descriptor is reachable"**,
   which `layout_note_slot` silently depends on. On a contradicting store to an
   object that is intact but descriptor-less, the probe resolves `None`;
   `layout_set_typed_unknown` — the only thing that clears the intact bit — is
   reached **only from the `Some(verdict)` arm** (`gc/layout.rs:782–788`), so
   control falls through to the pointer-mask path and **the bit is never
   cleared**. The object is thereafter `SIDE_MASK` to the collector and *intact*
   to the class-field inline guard, which consults no map by design. The raw-store
   fast path then writes a double over a pointer slot with no barrier and no
   layout note, and the next collection walks it as a heap pointer.

   Note the comment at `layout.rs:742` says a `None` verdict "can only cost an
   extra fall-through, never mis-track a slot". That is true **only while the
   invariant holds** — it is a consequence of it, not an independent guarantee,
   and it reads like reassurance to anyone implementing this.

**A declared class is no longer slower than an object literal.** #7512 is
**closed**: `churn_alloc` and `push_cls` both measure 0.75 s. The cause was not
diffuse — **#7515** fixed it, and the root cause is worth carrying forward
because it generalises: the dead-field-init elision matched `Expr::PropertySet`,
which the compiler *synthesizes* for anon-shape literal constructors, while
every source-level `this.v = v` lowers to `Expr::PutValueSet`. **Nothing a user
can type produces `PropertySet`**, so the elision was structurally unreachable
for the declared class it was documented as covering. *An unreachable predicate
passes every soundness test there is* — #7486 was correct in everything it
asserted and did nothing on the case it named.

One row in #7578 is **unexplained and should not be acted on as written**:
`js_array_length` at 10–15%, against a single call site executing 20,000 times
in a 20,000,000-push workload. Two isolation attempts failed — varying array
size moved the workload into a different GC regime, and a `.length`-only
microbenchmark measured 1.00 ns/iter for both a 1,000- and a 10-element array,
which is an empty loop (the read is loop-invariant and was hoisted). Both dead
ends are recorded on the issue.

**Workstream A has landed (#7566, v0.5.1324): the inline bump allocator is back
at `new` sites inside loop bodies**, outlined everywhere else.

| bench | outlined | gated | ceiling (all-inline) |
|---|--:|--:|--:|
| `churn_alloc` | 1.32 s | 0.73 s (**1.81x**) | 1.81x |
| `push_cls` | 1.30 s | 0.72 s (**1.81x**) | 1.78x |
| `churn` | 1.62 s | 1.04 s (**1.56x**) | 1.56x |
| `tree` | 8.97 s | 9.10 s (**0.986x**) | 1.05x |

It reaches the full unconditional-inline ceiling on the allocation-heavy shapes
at **+0 bytes for every site not in a loop** (verified by per-function IR
differential, not by assertion). `tree` pays **1.4%** — it allocates in loops so
it inlines, but its time is dominated by copying and promotion, so it takes the
bloat without the win. That is the honest cost of a static proxy.

**The measurement that justified the previous default had inverted, and nobody
had re-checked it.** The outlined form was made default on "−45 IR lines/site
AND ~17% faster"; the size half still holds (~268 bytes/site) but the speed half
is now **1.81x the wrong way**, because everything *around* the allocation got
cheaper (#7474, #7486, #7487, #7501, #7525, #7532, #7535, #7536, #7552) until
the surviving FFI call dominated what the inline bump's bloat costs. A default
chosen on a measurement is only as current as that measurement.

Recorded so it is not re-attempted: **Mach-O has no local-exec TLS model.**
Building the entire runtime with `-Ztls-model=local-exec` leaves the `blr`
through the TLV descriptor byte-identical (1.02x). Per-call cost is already at
the plain-global floor — only the *count* of resolutions can be reduced, which
is what inlining does and what #7565 did structurally.

**Two traps recorded here because they cost real time.** `PERRY_WRITE_BARRIERS=0`
**cannot** bound barrier cost — it makes `churn_alloc` *slower* (2.44 → 5.21 s)
because it also switches the collector out of evacuating mode; the 16.1% is
profile-derived only. And a TS annotation is never a layout fact, so no
bookkeeping may be elided because a field is declared `number` — elision must
be by-construction (`expr_produces_non_pointer_bits_by_construction`), and
#7501 found that even a static layout *declaration* gets revoked at runtime, so
collector-facing metadata needs a live header test at the store.

### Performance backlog — full app-pattern sweep (v0.5.1299, pinned quiet mini)

AC power, CPU-quiet gate passed, node 22.23.1 / bun 1.3.14, 11 runs per cell.
**All twelve kernels, worst first** — this supersedes the earlier partial table,
which topped out at 6.27x and predated two kernels running at all:

| kernel | perry | bun | node | perry/bun | owner |
|---|--:|--:|--:|--:|---|
| **object_deep_clone** | 657.0 ms | 17.5 | 56.9 | **37.5x** | ✅ **fixed — see below** |
| **promise_all_chains** | 259.7 ms | 22.7 | 64.0 | **11.4x** | unowned |
| json_parse_1mb | 438.2 ms | 68.1 | 127.1 | 6.4x | unowned |
| batch | 127.8 ms | 26.5 | 74.8 | 4.8x | unowned |
| map_1m | 1233.7 ms | 256.5 | 320.1 | 4.8x | ✅ **fixed — see below** |
| string_template_interp | 106.9 ms | 41.6 | 100.6 | 2.6x | unowned |
| json_stringify_1mb | 97.3 ms | 38.5 | 95.1 | 2.5x | unowned |
| string_concat_csv | 51.3 ms | 27.1 | 82.3 | 1.9x | borderline |
| buffer_transcode | 58.2 ms | 43.9 | 85.8 | 1.3x | ok |
| string_split_map_join | 51.1 ms | 44.1 | 75.8 | 1.2x | ok |
| regex_replace | 56.4 ms | 49.8 | 98.0 | 1.1x | ok |
| **date_format_parse** | 36.0 ms | 44.8 | 116.3 | **0.80x** | **win** |

**`object_deep_clone` and `promise_all_chains` are new to this table because
they were CRASHING, not slow** — fixed by #7495 and #7516/#7529. Their
first-ever measurement made deep clone the worst cell by 3x over the next.

#### Landed since that sweep — the two worst rows are gone

The table above is a single measurement *event* (v0.5.1299, 11 runs per cell)
and is kept intact as the baseline. The rows below were re-measured
individually on the same pinned quiet mini as part of the fix that moved them,
so they are **not** interchangeable with a fresh sweep — the artifact still owes
one, tracked under the #7475 blocker further down.

| kernel | sweep | now | vs bun | vs node | landed |
|---|--:|--:|--:|--:|---|
| **object_deep_clone** | 657.0 ms | **40 ms** | 37.5x → **~2.3x** | 11.5x → **0.67x (win)** | #7540 (closes #7533) |
| **map_1m** | 1233.7 ms | **309.1 ms** | 4.8x → **1.40x** | 3.9x → **0.96x (win)** | #7561 |

Both were single structural defects rather than broad slowness, which is why
each moved by an order of magnitude instead of a few percent:

- **deep clone** — `[...arr]` on an ordinary dense array ran the full iterator
  protocol. 90.45% of the whole process sat in `array_from_spread_value`, and
  the identical copy spelled `Array.from(tags)` cost **66x less**. A 3-element
  spread was ~25 allocations against bun's one allocation and a 24-byte memcpy.
- **map_1m** — `for (const v of m.values())` was 512 ms of the kernel; as a
  delete-safe index walk over the flat entries it is **5 ms**. Perry already won
  lookup before the fix (76 ms vs node 115, bun 103).

**Read this section before picking up any row above.** Four times this campaign
a ticket was worked from a headline number that had already collapsed — #7510
(33.6% → 11%), `layout_forget_object` (14.5% → 3.0% → 1.7%), `layout_note_slot`
(7.5% → 0.03%, correctly closed with **no code at all**), and #7478 (a 2.3x scan
penalty that was 1.01x by the time anyone re-ran it). Re-measure the row before
profiling it.

#7478 adds a second failure mode to watch for: **a stale floor is as
misleading as a stale headline.** Its acceptance bar was "materially under the
1350 ms `idiomatic` row" — but `idiomatic` is a *measurement*, not a constant,
and it had moved to 938 ms. A bar quoted as a number silently becomes a bar
quoted against a different build. When acceptance is "beat arm X", re-measure
arm X in the same run, never carry its number forward.

### JSON polyglot legs — the tape's scan penalty is closed (#7478)

`roundtrip` is the crown jewel and `field_access` was the standing problem: at
the v0.5.1299 sweep the optimized configuration was 2.2x SLOWER than the
unoptimized one at 3.6x the RSS, with a sigma of 136 against every other row's
under 5.

| leg | perry optimized | perry idiomatic | bun | node | rust serde_json |
|---|--:|--:|--:|--:|--:|
| roundtrip | **192 ms** (82 MB) | 1307 ms | 216 | 379 | 178 |
| field_access | **2984 ms** (219 MB, sigma 136) | **1350 ms** (61 MB) | 218 | 380 | 183 |

**That inversion is closed** — #7483 (DirectParser float parity), #7499
(reparse-on-materialize), #7537 (early batch flip), #7539 (tape
side-allocation), with #7546 fixing the wrong-JSON defect the work surfaced.
Re-measured at **v0.5.1370** on the pinned quiet mini, 11 interleaved rounds,
every arm's checksum identical to node 26.5.1:

| phase | tape on, then → now | tape off, then → now |
|---|--:|--:|
| parse only | 210 → **160 ms** | 1220 → 1196 ms |
| parse + full scan | 3030 → **1233 ms** | 1287 → 1224 ms |
| parse + stringify | 254 → **171 ms** | 1756 → 1449 ms |
| field_access | 2981 → **1721 ms** | — → 1480 ms |

The issue's headline was that a full scan cost **2.3x the direct parser**
(3030 vs 1287). It is now **1.01x** (1233 vs 1224): the tape neither wins nor
loses a scan, which is the correct resting state for a structure whose whole
value is on the paths that skip materialization. `roundtrip` — the memcpy path
this must not regress — went the other way, 254 → 171 ms.

**The two remaining terms are now orthogonal, which is the real result.** The
issue documented an *interaction* (scan sigma 214.9 under gen-GC vs 8.8 under
mark-sweep for the identical tape). Measuring all four `PERRY_JSON_TAPE` x
`PERRY_GEN_GC` combinations, `field_access` decomposes additively:

| | tape on | tape off | tape term |
|---|--:|--:|--:|
| gen-GC | 1721 ms | 1480 ms | **+241** |
| mark-sweep | 1133 ms | 938 ms | **+195** |
| gen-GC term | **+588** | **+542** | |

The tape costs ~200 ms whichever collector runs, and the collector costs
~560 ms whether or not the tape exists. The ~200 ms *is* the tape build (parse
only is 160 ms) and is structural, exactly as #7537 recorded: the build is
purely additive whenever the whole tree ends up materialized anyway, and
nothing can predict scan-shaped access before the parse.

So `field_access`'s remaining ~560 ms is a **generational-collector** term that
the tape-off arm carries identically. It is not a tape-policy problem and does
not belong to #7478; it is the same collector behaviour the GC campaign is
already working, on a workload that happens to reach it through `JSON.parse`.

### Gates and blockers

- **#7475 is the sole blocker for the public benchmark artifact**: two
  app-pattern kernels fail only under the auto-optimize runtime archive
  (isolated to the feature-stripped `.a`, scale-dependent, pre-existing).
  Until the artifact regenerates, `lint`'s public-baseline check stays red
  and merges to `main` need admin bypass.
- ~~#7477 DirectParser float divergence~~ — **fixed** (#7483, single
  correctly-rounded division per Clinger; all three of `PERRY_JSON_TAPE=0`,
  `=1` and node produce the same checksum). #7478 is unblocked.
- **The statepoint lowering has no static root-dominance checker.** The
  restored gates (#7452, #7460) verify the shadow-stack lowering only; the
  checker anchors on `@js_shadow_slot_bind`, which statepoint IR does not
  emit. Named at the call sites rather than papered over with a lowered floor.
- **Ratchet probe coverage gap**: all GC-ratchet probes run at the default
  nursery cap; a large-Eden arm would have caught both #7472 and the #7481
  residual.

---

## What is left, in order

1. **#7533 — `object_deep_clone` at 37.5x bun**, the worst cell in the public
   artifact by 3x and newly measurable (it used to crash). Profile FIRST; the
   issue carries an explicit A/B against `f06270d06` to settle whether today's
   rooting re-reads are material on a spread-heavy workload. If they are, the
   answer is hoisting them (#7487's pooled-alloca precedent), never removing
   them — they close real use-after-frees.
2. ~~**#7478 — the JSON tape's scan path**~~ — **closed, re-measured at
   v0.5.1370.** The headline (a full scan costs 2.3x the direct parser) is
   gone: 3030 → 1233 ms against the tape-off arm's 1224, i.e. **1.01x**, and
   `roundtrip` improved 254 → 171 ms rather than paying for it. The four-way
   `PERRY_JSON_TAPE` x `PERRY_GEN_GC` decomposition above is what closes it —
   the tape term (~200 ms, its own build) and the collector term (~560 ms) are
   now **additive and independent**, where the issue had documented them as an
   interaction. The residual on this benchmark is therefore the collector's,
   carried identically by the tape-off arm, and is tracked with the GC work
   rather than here.

   Worth recording as method: **this ticket's headline was stale in both
   directions.** The 2.3x had been fixed by four merges nobody had re-measured
   together, and its stated floor (the 1350 ms `idiomatic` row) had itself
   moved to 938 ms — so coding to the ticket would have chased a target that
   had already moved and declared failure against a bar that no longer existed.
   Two switches, measured one at a time, was the whole difference.
3. ~~**`_tlv_get_addr` — thread-local addressing**~~ — **measured out (#7565).**
   Re-measuring first is what decided the design: the 27.0% was real, but the
   ticket's "41 distinct call-graph sites, this is diffuse" was not — **seven
   functions carried 98% of it and every one resolved `tls_hot::HOT`**, two of
   them resolving nothing else. So the lever was the accessor, not the call
   graph. Publishing the address cache into a pthread TSD slot and reading it
   inline off `TPIDRRO_EL0` (how `pthread_getspecific` itself works; what
   mimalloc does here) took `_tlv_get_addr` to **1.1%** and bought
   `churn_alloc` **1.167x**, `churn` **1.175x**, `push_cls` **1.144x** on the
   pinned host, without touching a line of generated code — the ticket's
   "thread a context pointer through generated code" would have crossed every
   FFI boundary against 2994 `.with()` sites. What remains is
   `RuntimeHandleScope`, not the allocation path, so **the ceiling on further
   thread-local work here is ~1%**. #7469's other workstreams (codegen emitting
   the bump allocation inline; per-object footprint) are untouched.

   **#7510 is effectively closed.** All three items were measured out rather
   than argued away: item 1 shipped (#7535, install now 1x per 20M
   constructions), item 2 shipped (#7525), and **item 3 collapsed to 0.03%** —
   2 samples of 5,869, with codegen emitting *zero* `js_gc_note_slot_layout`
   sites for `churn_alloc` and a stub-it-entirely ceiling of 1.016x. The
   type-propagation work (#7550/#7552) plus declaration-at-allocation
   (#7501/#7532) removed the calls before anyone optimised them.

   **Three times this campaign a ticket's headline number was stale by the time
   it was worked** (#7510's 33.6% -> 11%, `layout_forget_object`'s 14.5% ->
   3.0% -> 1.7%, item 3's 7.5% -> 0.03%). Re-measure before scoping; a profile
   more than a few merges old sends people at the wrong thing.
4. **#7511 — write barriers (16.1%)**. Correctness-first: acceptance requires
   `PERRY_GC_VERIFY_EVACUATION=1` / `PERRY_GC_VERIFY_MARK=1` and the ratchet
   probes, because a wrong answer here corrupts memory rather than slowing it.
5. **#7502 — the shipped root lowering has no coverage**: nine mechanics have
   no native-roots assertion anywhere, six of them shapes
   `gc-rooting-invariant.md` records as having already shipped broken. Today's
   ~20 rooting bugs were all found by hand with `PERRY_GC_PROTECT_FROMSPACE`
   because nothing else can find them. This is the structural fix.
6. **Repsel** — the element-shape invariant landed (#7496) and its
   versioned-loop consumer landed (#7612, matrix #7608, corpus #7619). What
   remains of #7480 is **element `Ptr<Shape>` for object-literal element
   types**, and it is the whole measured gap, not a tail: #7612's
   `element_class_name` resolves `Array(Named(C))` only, so #7480's own kernel
   (`keep: {v,w}[]`) never reaches the clone. Re-measured 2026-08-08 on the
   pinned quiet host, 200k elements × 50 sweeps, checksums equal, interleaved:

   | kernel | perry | node | bun | ratio |
   |---|--:|--:|--:|--:|
   | `keep: {v,w}[]` — #7480's kernel | 414 ms | 12 ms | 12 ms | **34.5×** |
   | `keep: Node[]` — what #7612 covers | 13 ms‡ | 57 ms† | 15 ms | **0.23×** |

   **The issue's recorded 93 ms / 6.2× is stale and was optimistic** — the
   fourth time a figure in this plan has gone stale, and the first in that
   direction. Two traps in this one table, both of which cost time to find:

   †Node is *slower* on the named-class arm than on the literal arm (57 vs
   12 ms). `v: number;` cannot be erased by type stripping — without the
   annotation it is still a valid class field declaration — so V8
   pre-initialises the slot to `undefined`, pinning the field to tagged
   representation. **"Beat node" is therefore a different number on the two
   arms**; re-measure the arm you are gating on, in the same run.

   ‡That cell did not exist before #7660: on `main` the named-class arm
   *SIGBUSes*, because the versioned-loop consumer derived its elements base
   from an unresolved growth-forwarding stub for any array grown outside the
   scope that reads it. Step 2's win was real but gated behind a crash.

   The IR does not match the issue's recorded cost model either. #7480 says
   the body has "no out-of-line guard calls, the cost is stacked inline
   diamonds"; the object-literal arm actually carries **three calls per
   iteration** — `js_typed_feedback_observe_property_get`,
   `js_typed_feedback_record_guard_pass` (on the *hit* path) and
   `js_dynamic_string_or_number_add`, because with no resolvable class the
   field read falls into the by-name PIC tower and the accumulator loses its
   numeric proof. That is a second, separable lever, and it is the kind of win
   that gets misattributed to the element-shape work.

   Sequencing note carried from before: element reads are 13% of `churn` at
   4.3×, so against the bookkeeping levers this stays an RSS/footprint play
   more than a time one — but the 34.5× above is the kernel, and it is real.
7. **Layer 1** — migrate remaining lowerings onto the rooted-combinator API
   (`crates/perry-codegen/src/rooting.rs`). **#7615 is the ordered worklist**:
   88 modules, 694 raw-pointer sites, 262 hazard sites, grouped into ten
   slices by hazard density. A slice finishes by adding its modules to
   `MIGRATED_MODULES`, which denies `expr::temp_root` in them. The terminal
   condition is `expr/temp_root.rs` going `pub(in crate::rooting)` — the raw
   accessor unreachable, not merely uncounted. **Layer 3** — shrink the
   107-module ceiling list toward empty; same end state, same reason.
8. **Statepoint-side static checker** — teach `gc_root_dominance_check.py` to
   read relocation bundles, closing the gap the #7452/#7460 repairs named.
9. ~~**RSS re-derivation under the statepoint default** (#7056)~~ — **done, and
   the answer is that the root lowering is not an RSS lever.** Re-derived at
   `7bde3de24` on the pinned quiet mini (95% idle, zero `rustc`/`cargo`
   throughout), 12 ratchet probes x 7 repeats x **3 interleaved rotations**,
   every probe byte-identical to the pinned Node oracle in all 72 probe-runs:

   | | statepoint (default) | shadow stack (`PERRY_RS4GC=0`) | ratio |
   |---|--:|--:|--:|
   | peak RSS, 12 probes | 544.2 MB | 545.1 MB | **1.002** |
   | retained heap after `gc()` | 114,594,904 B | 114,596,520 B | **1.000** |
   | wall | 4,808 ms | 4,894 ms | 1.018 |

   **104 of 108 deterministic cells are bit-identical between the two
   lowerings.** All four that move are on `10_store_receiver_across_alloc` and
   are ≤0.31% — the shadow stack keeps 7 more objects live at that collection
   point than the statepoint map does. Between-rotation spread: retention
   **0.000%**, peak RSS max 0.621% (median 0.000%), wall max 2.779%. The arms
   were shown to differ before they were compared, per probe, not per suite:
   `statepoint-example` + `addrspace(1)` roots present on 12/12 under the
   default and **0 of both** under `PERRY_RS4GC=0`, with `js_shadow_frame_enter`
   rising to match.

   So #7056's RSS numbers were not invalidated by #7370. What *did* invalidate
   parts of it is everything else that shipped since, and the re-derivation
   should be read for those:

   - **§9's recommendation shipped** (#7377): the 16 MB cap no longer hangs off
     `gc_moving_loop_polls_enabled()`, and the poll has been default-off since
     #7161. The five arms #7056 tabulates (`on`/`off`/`off_rt`/`on_cap128`/
     `off_scav`) no longer name anything that ships.
   - **The cap is still the whole footprint lever, and now measured on 12
     probes rather than 8**: at `PERRY_GC_SCAVENGE_NURSERY_MB=128`, peak RSS is
     **1.911x** and retained heap **2.475x** the shipped cap, for **0.947x** the
     wall — up to 5.005x peak RSS on `04_dead_after_deep_stack` and 6.350x
     retention on `07_array_grow_evacuate`. Identical to 0.6% under both root
     lowerings, so this is a pacing result and not a rooting one.
   - **§7's false-retention table is gone, not shrunk.** It recorded a
     4.6x–54.8x conservative-vs-precise band on probes 01–08. `classify` at
     `7bde3de24` reports **excess 0.00% and spread 0 on all twelve**, with no
     `manual_collect` scan site anywhere — #7657 removed the forced scan at
     `gc()` rather than the reading.
   - **§6's real finding survives intact**: a conservative scan does not
     degrade the copying minor, it **disables** it. Forcing
     `PERRY_CONSERVATIVE_STACK_SCAN=full` under statepoints takes
     `minor_cycles` to **0 on all twelve probes** (`copied + promoted` 0 on
     all twelve), costs 1.98x retained heap (0.70x–6.35x) and 1.46x peak RSS
     (0.93x–3.47x). Smaller than the +364%..+5371% #7056 recorded, same sign,
     same mechanism.

   Not re-derived, and stated rather than hidden: #7056's largest RSS numbers
   (§3–§5, 272 MB -> 421 MB) came from three server-shaped workloads it wrote
   and never landed, so there is nothing in the tree to re-run. The 12 probes
   are what replaced them.
10. ~~**Ratchet large-Eden probe arm** (#7481's lesson), plus the pending
   quiet-host re-pins (`wt-scavtenure` baseline)~~ — **both done.**

    `13_large_eden_survivors` pins the cadence the matrix had no coverage of,
    via a per-probe `// gc-ratchet-env:` declaration that `check` compares like
    a metric (delete the directive and every band is still satisfied, which is
    exactly why the arm itself has to be gated). Full rationale and the
    perturbations that turn it red: `benchmarks/gc_ratchet/README.md`.

    **The finding that shaped it is worth carrying forward.** A large Eden on a
    small retained set runs **zero copying minors** —
    `arena_growth_full_escalation_due` escalates every minor to a full
    mark-sweep once arena in-use clears its 32 MB floor and exceeds twice the
    post-full baseline, which a 64 MB Eden over a ~1 MB live set does every
    time. The first draft of the probe did precisely that and would have been
    pinned on a collector it never reached. So `PERRY_GC_SCAVENGE_NURSERY_MB`
    is not, on its own, a "larger Eden" knob: above ~32 MB it is a "no
    copying minor" knob unless the workload also holds a live set. Sizing the
    retained set to 262,144 objects buys 4 copying minors freeing 37/36/68/68 MB
    (the first copies 532,482 objects in one cycle) against ~16 MB per minor
    for every default-cap probe.

    **`wt-scavtenure` is subsumed, measured rather than assumed.** #7432 is
    merged, its worktree exists on neither host, and the baseline has been
    re-pinned five times since — most recently in full by #7657 at `59d522052`
    on the pinned host, which also closed #7652's mixed provenance. Running
    `check` at `7bde3de24` against that artifact on the pinned host: **`OK`**,
    12/12 probes, `pinned_host` profile. The only cell outside its band is
    `09_try_catch_roots.heap_used_bytes` at **−17.98%**, which is an
    *improvement* (one 1 MiB block of #7559-shaped residue) and is folded into
    this re-pin.

---

## Binding rules (distilled from incidents; provenance in the history doc)

- **Measure on a quiet host.** The sweep's own gate (≤25% CPU sustained for
  60 s, AC power) is the standard. A fix was once reverted because the host
  was at load 55 and its check matched an absent symbol.
- **The #6377 gate:** every "more type visibility" change un-gates latent
  broken fast paths its own microbench never exercises. Acceptance for any
  repsel/proof phase is the FULL gap suite against a same-session `main`
  baseline, byte-diffed against the pinned node oracle — never the phase's
  own microbench.
- **Stale-archive discipline:** `perry-runtime`/`perry-stdlib` are rlib-only —
  build the `-static` wrappers, verify the `.a` mtime moved, and set
  `PERRY_NO_AUTO_OPTIMIZE=1` for hand-rolled probes. The auto-optimize path
  builds its own feature-stripped runtime and links it OVER
  `PERRY_RUNTIME_DIR`, which silently voids A/B tests (and is itself the
  subject of #7475).
- **A gate must assert its subject was live**: zero root stores ⇒ refuse the
  verdict; count the corpus; sabotage-test new instruments (plant the bug,
  watch the gate go red). Four required gates were dead on `main` in one day
  for violations of exactly this.
- **Do not** re-measure GC pacing or update the README's performance table
  mid-cycle. GC env knobs follow CLAUDE.md's kill-policy.
- **`$?` after a pipe is the pipe's exit status, not the program's.** Capture
  exit codes without pipes; this produced both a false red and a false green
  in a single afternoon.
