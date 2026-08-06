# Perry engine plan — status quo and what is left

**Goal (owner):** best performance, best RSS footprint, minimal binary size.

**Tracker:** #7294 (routing only — this document is authoritative). **History:**
every dated status section, incident narrative and superseded sequencing lives
in [`engine-plan-history.md`](engine-plan-history.md); this file holds only the
current state and the remaining work so it stays readable across context loads.
Last synced **2026-08-06** (repsel status corrected same day: 4b was already merged).

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

### Performance backlog (public-baseline sweep, 2026-08-06, quiet host)

Pinned Node v22.23.1 / Bun 1.3.14, 11 runs per cell. Worst-first vs bun:

| row | ratio | workstream |
|---|--:|---|
| `json_parse_1mb` | 6.27× | JSON parser speed (shared with tape) |
| `map_1m` | 5.74× | Map internals |
| `batch` | 5.29× | app-pattern batch |
| `field_access` (json_polyglot) | ~13× | JSON tape scan — #7478, **now unblocked** (#7477 fixed by #7483) |
| element-shape kernel (`keep[j].v`) | 6.2× vs node | repsel #7480 |
| `string_template_interp` | 3.09× | string building |
| `json_stringify_1mb` | 2.62× | JSON stringify |

**Wins to protect:** JSON roundtrip **194 ms vs bun 221 / node 384** (the
tape's memcpy path — any tape change is measured against it; #7476 pins the
numbers); `date_format_parse` 0.82× vs bun.

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

1. **#7475** — fix the auto-optimize-archive failures, rerun
   `benchmarks/run_public_baseline.sh`, publish the artifact. This also turns
   `lint` green and ends admin-bypass merging.
2. **JSON workstream** — re-land #7478's reparse-on-materialize (now
   unblocked by #7483; projected ~1500 ms field_access from 2981 while
   keeping the roundtrip win). The measured dead ends are recorded in #7478 —
   do not re-walk them.
3. **Repsel** — build the #7480 element-shape invariant → versioned-loop
   consumer → element `Ptr<Shape>`. This is the dependency chain for the
   follow-on optimization work (4b itself is done — #6919).
4. **Layer 1** — migrate remaining lowerings onto the rooted-combinator API
   (`crates/perry-codegen/src/rooting.rs`); the arm-aware scan is the
   worklist tool. **Layer 3** — shrink the 107-module ceiling list toward
   empty; the end state is the raw accessor unreachable, not counted.
5. **Statepoint-side static checker** — teach `gc_root_dominance_check.py` to
   read relocation bundles, closing the gap the #7452/#7460 repairs named.
6. **RSS re-derivation under the statepoint default** (#7056) — the earlier
   numbers were measured under the shadow stack.
7. **Ratchet large-Eden probe arm** (#7481's lesson), plus the pending
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
