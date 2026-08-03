# Perry engine plan — correctness and performance, one document

**Goal (owner):** best performance, best RSS footprint, minimal binary size.

**Tracker:** #7294 (routing only — this document is authoritative).

This is the single entry point. It replaces five overlapping documents and a
55 KB uncommitted working file. Detail lives in linked RFCs; **sequencing and
rationale live here**.

| Concern | Detail lives in |
|---|---|
| GC rooting correctness | [`src/internals/rfc-rooting-by-construction.md`](src/internals/rfc-rooting-by-construction.md) |
| The rooting invariant + checker blind spots | [`src/internals/gc-rooting-invariant.md`](src/internals/gc-rooting-invariant.md) |

---

## Part 1 — GC correctness

**The shape, stated once:** *a GC-managed pointer exists somewhere the collector
does not know about, across a point where the collector can run.*

40 GC/rooting commits landed in three days and the blocking bug (#7280) still
measured red 0/30. Every fix was correct; none ended the class — because the
pointer has **three different homes**, each needing a different mechanism.

| Layer | Home | Example bugs | Mechanism | Status |
|---|---|---|---|---|
| **0** | *enabler* | — | **in-process LLVM** (#7241) | Phase 0 done |
| 1 | `perry-codegen` lowering code | #7192, #7206, #7211 | `Raw`/`Rooted` borrow discipline | proposed |
| 2 | emitted code's liveness | #7280, #7271, #7252, #7243 | statepoints (#7108, #7174) | blocked on 0 |
| 3 | `perry-runtime` hand-written Rust | #7249, #7239, #7226, #7231 | `RuntimeHandleScope`, non-optional | not started |

**Order is 0 → 2. Layers 1 and 3 are independent and can proceed now.**
#7108 measured statepoints viable but blocked: *"the text-IR-plus-stock-clang
architecture is what rules the cheapest design out."* #7241 removes exactly that
and independently verified `gc "statepoint-example"` constructs, verifies, emits.

**Costs, so they are decided rather than discovered.** Stack maps: 438,848 B hot
text saved for **4.5–16.6 MB** cold metadata. It is cold, so RSS cost ≪ file-size
cost, and `24 B × (safepoint, root) pairs` over 62,731 candidate safepoints makes
**safepoint density a lever — expected, not measured. Layer 2 must prove it
first.** In-process LLVM: ~171 MB static-linked when enabled, zero by default.

**RSS interaction.** The −65% (320 MB → 111 MB) comes from the **16 MB nursery
cap**, not the copying minor — they merely share a flag. A no-poll arm reaches the
same 108 MB. **Sequenced last deliberately**: the "20× wall cost" was measured
while #7255's defect made "minors" fall back to a conservative full scan. Minor-GC
cost should scale with *survivors*, not collection count, so 20× is a symptom.
Re-derive after Part 1 lands. See #7056.

---

## Part 2 — Performance

### The framing (unchanged, and now confirmed)

Perry does not NaN-box eagerly *by choice*. NaN-boxing is the **fallback**, and
correct for genuinely polymorphic values. The problem is that **the proofs that
would let us stop almost never succeed**, so the fallback is what everything gets.
The machinery exists and does not fire.

> **The fix is in the proofs, not in the value representation.**

**2026-08-03 confirmed this precisely.** The three worst benchmarks lose on a
*missing proof*, not a missing representation — see #7286 below.

### What is measured, and what it cost to learn

**Per-site win is large.** Step 0 (quiet M1 mini, replicated on Pi 5, interleaved,
instructions retired, byte-exact vs Node): **−19.4%** is the defensible per-site
figure; a field-traffic loop hit −84% but partly against a fast path that was not
firing. **Coverage, not sharpening, is the binding constraint.**

**Coverage work then measured net ~0%** (#7128), with one +14.87% regression
(mandelbrot, fixed by #7132's profitability gate). The only real win was canonical
`Str` at −4.12% — earned by **deleting two opaque runtime calls per iteration**,
not by changing storage. `-O3` already achieved most i32 promotions.

> **⇒ The scoreboard is opaque `js_*` calls removed from hot paths — never
> promotion counts.** That metric would have predicted the null in advance.

**A promotion goes unconsumed three ways**, all found the hard way: a context gate
refuses it, `escape_news.rs` scalar-replacement deletes the object, or the clones
are dead-stripped for having zero call sites. **Verify consumption in emitted IR
with call sites checked** — object hashes and counters both lie.

**Architecturally correct coverage** means all of: every representation fires
wherever it *soundly* can; promotion gated on **benefit**, not provability alone;
each knob isolates exactly one representation; the instrument distinguishes
selected / consumed / scalar-replaced / denied-with-reason; and **no
representation is dead** (three currently are — `Ptr<NumArray>` emits nothing,
`canonical-u32` is 0/18).

### The measured levers (2026-08-03, release, auto-optimize ON)

| Benchmark | Perry | Node | With lever | Issue |
|---|---:|---:|---|---|
| `matrix_multiply` | 631 ms | 32 ms | **59 ms (10.7×)** | #7286 |
| `prime_sieve` | 107 ms | 5 ms | **27 ms (4.0×)** | #7286 |
| `method_calls` | 79 ms | 10 ms | ~9× available | #7287 |

**The discriminator is heap access, not arithmetic.** Perry is at parity or ahead
whenever the hot loop's live set is entirely scalar locals (`mandelbrot` 22 vs 24,
`fibonacci` 387 vs 908). It loses 8–20× the moment a hot value lives in a heap
cell — array element or object field.

**#7286 — the missing proof is not "is it i32".** `(i*size+k)|0` produces genuine
i32 and buys **nothing**, because `|0` has `min < 0`. What is missing is
**non-negativity plus an upper bound**. One unbounded numeric *parameter* demotes
every access in the function. Three levers: monotone-induction range for strided
counters, affine `a*b+c` proof, interprocedural range summaries for numeric params.

**#7287 contradicts the scoreboard, and that is worth knowing.**
`method_calls` has **zero `js_*` calls in its hot loop** — it is guard-bound, ~60
IR instructions of guard around 3 of work. It already scores perfectly on
"opaque calls removed" while sitting 7.9× behind. **The metric is necessary, not
sufficient.**

**#7288 — build non-determinism.** Byte-identical source → 78 ms or 3450 ms
depending on *where the `.ts` file lives*. Narrow blast radius (class-field-in-hot-loop
only), but it means one published figure reproduces only inside the checkout.

### Live tracks

- **Track E — make declared types load-bearing.** The structural version of
  #7286: a declared `number[]` should carry its own proof.
- **Track F — live-range splitting and type recovery for minified dependency JS.**
  Owner's framing: *not a de-minifier* — make minified code compiler-friendly and
  **find holes before we poke at them**. First step is a measurement, not a build.
- Dead representations: `Ptr<NumArray>`, `canonical-u32`.

---

## Sequencing

1. **Now, independent:** layer 1 and layer 3 rooting; #7286's index range proof.
2. **Next:** in-process LLVM (#7241) → statepoints (#7108/#7174).
3. **After the collector is trustworthy:** re-derive the RSS numbers (#7056).
4. **Do not** re-measure GC pacing, or update the README's performance table,
   mid-cycle.
