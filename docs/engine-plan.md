# Perry engine plan — status quo and what is left

**Goal (owner):** best performance, best RSS footprint, minimal binary size.

**Tracker:** #7294 (routing only — this document is authoritative). **History:**
every dated status section, incident narrative and superseded sequencing lives
in [`engine-plan-history.md`](engine-plan-history.md); this file holds only the
current state and the remaining work so it stays readable across context loads.
Last synced **2026-08-06**, after the v0.5.1299 public-baseline sweep.

| Concern | Detail lives in |
|---|---|
| GC rooting correctness | [`src/internals/rfc-rooting-by-construction.md`](src/internals/rfc-rooting-by-construction.md) |
| The rooting invariant + checker blind spots | [`src/internals/gc-rooting-invariant.md`](src/internals/gc-rooting-invariant.md) |
| Representation selection (unbox-by-default) | [`representation-selection-rfc.md`](representation-selection-rfc.md) |
| How each conclusion below was reached | [`engine-plan-history.md`](engine-plan-history.md) |

---

## Status quo

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
| `_tlv_get_addr` | 17.0% | #7469 structural half (partly the same thread-locals as #7510) |
| write barriers | 16.1% | **#7511** — *correctness-first: a missed barrier is a use-after-free, not a slowdown* |
| typed-feedback guards | 9.2% | repsel 3b |
| array helpers | 6.2% | partly closed by #7501 |
| **the actual allocation** | **7.7%** | — |
| user code | 3.7% | — |

A declared class costing **63% more than an object literal** is backwards and
is tracked as a suspected defect (**#7512**), with an explicit off-ramp: if the
cause lands in the layout or barrier subsystems it closes as a duplicate.

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
| **object_deep_clone** | 657.0 ms | 17.5 | 56.9 | **37.5x** | **#7533** |
| **promise_all_chains** | 259.7 ms | 22.7 | 64.0 | **11.4x** | unowned |
| json_parse_1mb | 438.2 ms | 68.1 | 127.1 | 6.4x | unowned |
| batch | 127.8 ms | 26.5 | 74.8 | 4.8x | unowned |
| map_1m | 1233.7 ms | 256.5 | 320.1 | 4.8x | unowned (largest absolute) |
| string_template_interp | 106.9 ms | 41.6 | 100.6 | 2.6x | unowned |
| json_stringify_1mb | 97.3 ms | 38.5 | 95.1 | 2.5x | unowned |
| string_concat_csv | 51.3 ms | 27.1 | 82.3 | 1.9x | borderline |
| buffer_transcode | 58.2 ms | 43.9 | 85.8 | 1.3x | ok |
| string_split_map_join | 51.1 ms | 44.1 | 75.8 | 1.2x | ok |
| regex_replace | 56.4 ms | 49.8 | 98.0 | 1.1x | ok |
| **date_format_parse** | 36.0 ms | 44.8 | 116.3 | **0.80x** | **win** |

**`object_deep_clone` and `promise_all_chains` are new to this table because
they were CRASHING, not slow** — fixed today by #7495 and #7516/#7529. Their
first-ever measurement makes deep clone the worst cell by 3x over the next.
#7533 carries the profile-first instruction and an explicit A/B against
`f06270d06` to establish how much predates today's rooting stack.

### JSON polyglot legs — the tape is a net negative on scans

Same run. `roundtrip` is the crown jewel and `field_access` is the problem:

| leg | perry optimized | perry idiomatic | bun | node | rust serde_json |
|---|--:|--:|--:|--:|--:|
| roundtrip | **192 ms** (82 MB) | 1307 ms | 216 | 379 | 178 |
| field_access | **2984 ms** (219 MB, sigma 136) | **1350 ms** (61 MB) | 218 | 380 | 183 |

Perry **wins roundtrip** against both JS runtimes and lands within ~8% of Rust
serde_json. But on `field_access` the **optimized configuration is 2.2x SLOWER
than the unoptimized one** and carries 3.6x the RSS — the lazy tape is a net
negative on scan-shaped access, and its sigma of 136 (against every other row's
under 5) says the cost is data-dependently variable. The 1350 ms idiomatic row
is the floor any fix must beat: simply declining the tape for this shape would
already be 2.2x better. Tracked in **#7478**.

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
3. **`_tlv_get_addr` — thread-local addressing, now 30.5% of `churn_alloc`**
   and the largest single line in the profile. This is #7469's structural half
   (a context pointer, or inlined bump-allocation in generated code); #7474's
   hot-TLS cache took it from 34% to ~14–17%, and it has climbed back as a
   *share* because everything around it shrank, not because it regressed.

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
