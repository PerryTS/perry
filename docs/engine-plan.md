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
The last unstarted track is the **Layer-1 emitter migration**. The v0.5.1299 public-baseline sweep is
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
Layer-1 emitter migration (not started).

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
| **1** | `perry-codegen` lowering code | `Raw`/`Rooted` discipline | design **validated & corrected** (#7459 — the RFC's own constructor was `E0499`); combinator form proven on the real emitter (#7461); the raw-pointer-across-lowering bug shape **eliminated crate-wide** (#7453, #7462–#7465); full emitter migration **not started** |
| **2** | emitted code's liveness | statepoints | ✅ **the default**, target-aware (#7370): native roots where the runtime can walk frames, shadow stack elsewhere |
| **3** | `perry-runtime` hand-written Rust | `RuntimeHandleScope`, non-optional | per-module ceilings (#7457): **595 of 705 modules locked at zero**, 107 listed with ceilings, 999 sites, and the list can only shrink — a cleaned module cannot regress (#7458). `across_*` combinators are the prescribed form (#7455). **End state not reached:** the raw accessor is still reachable inside listed modules |

### Repsel stack (the unbox-by-default campaign)

Phases **1 / 2 / 3a (#6909) / 3b (#6911) / 4a (#6915 + #7421/#7425) / 4b
(#6919)** are all merged; #6904's 26× histogram is closed (#7485 deleted the
dead 4b prototype flag). Next gap:
**element-shape proofs through array reads** — `keep[j].v` measured **6.2× vs
node** on the pure shape — route decided in **#7480**: both candidate routes
share one prerequisite (a per-array homogeneous-element-shape invariant,
construction-maintained, self-healing like 4a's dense bit), consumed first by
the #5093 versioned-loop clone, then by element `Ptr<Shape>`.

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

**Read this section before picking up any row above.** Three times this campaign
a ticket was worked from a headline number that had already collapsed — #7510
(33.6% → 11%), `layout_forget_object` (14.5% → 3.0% → 1.7%), and `layout_note_slot`
(7.5% → 0.03%, correctly closed with **no code at all**). Re-measure the row
before profiling it.

### JSON polyglot legs — the tape is a net negative on scans

Same run. `roundtrip` is the crown jewel and `field_access` is the problem:

| leg | perry optimized | perry idiomatic | bun | node | rust serde_json |
|---|--:|--:|--:|--:|--:|
| roundtrip | **192 ms** (82 MB) | 1307 ms | 216 | 379 | 178 |
| field_access | **2984 ms** (219 MB, sigma 136) | **1350 ms** (61 MB) | 218 | 380 | 183 |

Perry **wins roundtrip** against both JS runtimes and lands within ~8% of Rust
serde_json. `field_access` was the standing problem: the optimized configuration
was 2.2x SLOWER than the unoptimized one at 3.6x the RSS, with a sigma of 136
against every other row's under 5.

**That inversion is now closed** (#7478 → #7537 early batch flip, then #7539
tape side-allocation), measured on the same host:

| `field_access` | median | sigma | peak RSS |
|---|--:|--:|--:|
| sweep (v0.5.1299) | 2984 ms | 136 | 219 MB |
| after #7537 | 2043 ms | 146 | 195 MB |
| **after #7539** | **1809 ms** | **17.3** | **155 MB** |

Sigma collapses **8.3x** — that was the headline symptom, not the median — and
the decisive result is that **turning the tape ON is no longer worse than
turning it OFF**. The tape-off arm still carries sigma 117.2 and 168 MB, so the
residual variance and footprint belong to the generational collector's behaviour
on this workload and have nothing left to do with the tape. The GC trace agrees
to the cycle: 19 cycles / 9 full / 6 `old_gen_bytes` becomes **14 / 5 / 2**,
which is the `PERRY_JSON_TAPE=0` arm's profile exactly.

`roundtrip` — the memcpy path this must not regress — **improved**, 201 → 193 ms,
with peak old-generation in-use down 39.6 → 14.1 MB.

Still open: 1809 ms has not reached the 1350 ms idiomatic floor, and the gap is
now collector behaviour rather than tape design.

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
2. **#7478 — the JSON tape's scan path**, where our optimized build is 2.2x
   slower than our unoptimized one. The 1350 ms idiomatic row is the floor.
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
6. **Repsel** — the element-shape invariant landed (#7496); the versioned-loop
   consumer and element `Ptr<Shape>` remain. Deliberately sequenced **after**
   the bookkeeping levers: element reads are 13% of `churn` at 4.3×, the best
   ratio in the table, so this is an RSS/footprint play more than a time one.
7. **Layer 1** — migrate remaining lowerings onto the rooted-combinator API
   (`crates/perry-codegen/src/rooting.rs`); the arm-aware scan is the
   worklist tool. **Layer 3** — shrink the 107-module ceiling list toward
   empty; the end state is the raw accessor unreachable, not counted.
8. **Statepoint-side static checker** — teach `gc_root_dominance_check.py` to
   read relocation bundles, closing the gap the #7452/#7460 repairs named.
9. **RSS re-derivation under the statepoint default** (#7056) — the earlier
   numbers were measured under the shadow stack.
10. **Ratchet large-Eden probe arm** (#7481's lesson), plus the pending
   quiet-host re-pins (`wt-scavtenure` baseline).

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
