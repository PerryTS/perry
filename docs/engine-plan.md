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
| **0** | *enabler* | — | **in-process LLVM** (#7241) | ✅ **landed** (#7301) |
| 1 | `perry-codegen` lowering code | #7192, #7206, #7211 | `Raw`/`Rooted` borrow discipline | proposed |
| 2 | emitted code's liveness | #7280, #7271, #7252, #7243 | statepoints (#7108, #7174) | ✅ **landed opt-in** (#7314) |
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

## ★ Status 2026-08-03 — the architecture is proven, not yet adopted

**Layers 0 and 2 have landed.** #7301 put the LLVM pipeline in-process; #7305
replaced setjmp/longjmp with `invoke`/`landingpad`, which is what makes try-
carrying functions statepoint-able at all; #7314 landed native-frame GC roots via
`gc.statepoint`, **opt-in behind `PERRY_STATEPOINTS=1`**, with the default path
byte-for-byte unchanged.

**What #7314 actually establishes:**

- **Every root path fails closed.** The plain `llvm.experimental.stackmap`
  lowering is *deleted*, not kept as a fallback — it survived in three places
  that all failed **open**, one of them dead by construction. LLVM may record a
  root slot as `Register R#N` (caller-saved, unrecoverable): measured **3 of 60
  locations** on one probe.
- **The metadata objection is answered.** #7108's headline cost was 4.5–16.6 MB.
  Re-encoding at assembly time gives **4,214,384 → 224,832 B (18.7×)**. The
  dominant lever is that **77% of records carry the same live set as the record
  before them**, so a repeat flag replaces the payload.
- **Evidence**: 23,301 safepoints → 23,301 statepoints, **0 plain maps, 0 parser
  fallbacks**, 129,914 relocations, max 53 live roots at one safepoint; all three
  arms 9/9 against the pinned Node oracle, and the statepoint arms also under
  `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1`.

**What it does NOT establish — read before planning on it:**

- **Binary size is a wash, not a win** (+496 B bridge, +50,064 B RS4GC on
  drizzle). An earlier revision measured a 49–131 KB win; merging main moved the
  shadow baseline down further. Per the author: *closing that axis needs **fewer
  roots**, not a tighter encoding — 221 KB for 154k roots is near this format's
  floor.* Runtime −0.93%, RSS flat. **The case is correctness structure, not
  headline numbers.**
- **Statepoints do not cover layer 3.** They describe *emitted* frames. A
  `*mut ObjectHeader` in hand-written runtime Rust is invisible to LLVM, so
  #7231/#7249's class is untouched — and #7280's fault has already *moved there*
  (`js_native_call_method + 580`, a receiver stale in a runtime frame).

### Blocking adoption — concrete, and neither is code

1. ~~**Four of five new knobs have no CI arm.**~~ **Closed by #7319.** Every
   surviving knob now has an arm that asserts its own subject was live, and the
   fifth was deleted: `PERRY_STATEPOINT_REPORT` was a second spelling of
   `--statepoint-report`, so the env spelling is gone and the flag is the only
   entry point. `PERRY_RS4GC` asserts every function record carries
   `backend: rs4gc` (it bails per function to the explicit bridge, so a green
   9/9 matrix proves nothing on its own); `PERRY_GC_SAFEPOINT_ONLY` asserts a
   codegen differential (statepoints strictly down, skipped calls strictly up);
   `PERRY_STACKMAP_WALKER` asserts `fp_walks > 0` under `verify` and
   `fp_walks == 0` under `unwind`, from the GC trace.
2. ~~**#7314 broke the file-size gate.**~~ **Closed by #7319** — `function.rs`
   2036 → 952 (statepoint/RS4GC lowering into `function/precise_roots.rs`) and
   `linker.rs` 2082 → 1618 (unit tests into `linker_tests.rs`, the pattern that
   file already used for `linker_temp_lifecycle_tests.rs`). Verified by
   byte-identical emitted IR over 45 modules × 3 modes.
3. **`gc-native-roots` is not a required status check**, so it reports without
   blocking — CLAUDE.md hazard 2, the one that let #6925's regression survive
   three merges. **Still open, and the reason changed**: promotion waits on a
   green run, and the job had never been green *at all* (see below). Promote the
   fan-in context `gc-native-roots-complete`, not the individual arms.

### ★ Statepoints are aarch64-only today (#7321)

Found while building those arms, and it is the largest single correction to the
picture above. **`PERRY_STATEPOINTS=1` cannot compile one module on x86-64
Linux.** The compact-map rewriter refuses — *"this module emits an LLVM stack
map that the compact-map rewriter could not parse … Refusing to emit a binary
that would lose roots silently"* — on the **first** probe, which is why
`gc-native-roots` had failed every run since it was pointed at `ubuntu-latest`.
That is the fail-closed path working; the consequence is scope. #7314's evidence
(drizzle, 23,301 statepoints) is aarch64 evidence. `gc_map.rs` names its base
registers in aarch64 terms throughout, which is consistent, though not proven to
be the cause.

The matrix therefore runs on `macos-14`, and `statepoints-refuse-x86` pins the
refusal *as a refusal* and goes red the day x86-64 starts working.

**The blocker behind it is now measured (#7333).** Even once the map parses, the
walker cannot run: x86-64 roots are all `Indirect [RSP + off]` (DWARF 7), and
`_Unwind_GetGR(ctx, 7)` **segfaults** — not "returns something unreliable", which
is what this was previously assumed to be. On x86-64 Linux (glibc 2.39, gcc
13.3.0), RBX/RBP/RIP return correctly while RAX and RSP both SIGSEGV: libgcc
tracks only the columns CFI restores, and RSP is derived from the CFA rather than
tracked. So reg 7 is the one lookup guaranteed to fault, and it is the only one
x86-64 roots use.

The recoverable bases do exist — `_Unwind_GetCFA` works, RBP works, and every
generated function already carries `"frame-pointer"="non-leaf"` under native
roots. What is missing is the per-function delta to the body RSP the map's
offsets are relative to. The cheapest place to close it is the compact-map
rewriter (#7314), which already parses the emitted **assembly**, where the
prologue is visible — the same technique #7329 just corrected for the aarch64
fast walker, whose missing trailing `sub sp, sp, #imm` is the exact analogue of
x86-64's `sub rsp, N`. Doing it there keeps a second architecture-specific
prologue decoder out of the runtime.

A second latent defect, now fixed: the workflow set
`RUSTFLAGS="-Cforce-frame-pointers=yes"`, which **replaces** `.cargo/config.toml`'s
`[build] rustflags` wholesale and so dropped `-C force-unwind-tables=yes`. A/B'd
on one tree: without it `09_try_catch_roots` aborts outright and the platform
unwinder visits **zero** frames — so on any host where the x29 chain walk is
unavailable the native-root walker finds no roots, and forced evacuation stays
quiet because it enumerates through that same walker.

### ★ Update, later on 2026-08-03 — the two structural blockers are gone

Both were structural rather than numeric, and both are now closed. What remains
blocking adoption is scope (x86-64) and process (a required check), not design.

**1. There was no working statepoint path for `try` on a default toolchain
(#7339).** The explicit bridge cannot root an `invoke`, and since #7305 every
call inside a `try` *is* an invoke — so the bridge refuses those functions
outright (#7330). RS4GC handles them, but it ran as an external `opt` subprocess
whose output an older `clang` could not parse (`error: unterminated attribute
group`), making it reachable only on a hand-pinned LLVM 22. **128 of 479 gap
tests (26%) contain `try {}`**, so a quarter of the suite had no statepoint path
at all. Routing RS4GC through layer 0's in-process pipeline removes the external
boundary entirely: all nine probes now compile with no `PERRY_LLVM_*` pinning,
byte-identical to the shadow-stack control, copying 5,946–90,271 objects, with
`backend rs4gc` on every function record.

**2. "Delete the shadow stack, keep statepoints" was not expressible (#7340).**
The root-set *analysis* and its *lowering* were one knob, so
`PERRY_SHADOW_STACK=0 + PERRY_STATEPOINTS=1` disabled the analysis and left the
statepoint lowering with nothing to lower — a rootless binary that ran correctly
until a collection freed something live. #7332 made the pair a hard error; #7340
splits the predicate so the pair is *selectable*, with the knob proven inert
under statepoints (identical root map, identical `__text`).

That second one matters more than its diff suggests. **A mode nobody can select
is a mode nobody can measure**, and the reason the adoption question kept
stalling is that its central configuration could not be run.

**What this changes about the decision below:** the earlier soak's verdict —
13 gap regressions, "do not flip" — was measured against the *bridge*, before
#7329/#7330, and against a backend that structurally cannot compile a quarter of
the suite. **It should not be carried forward.**

Re-measured 2026-08-03 against RS4GC in-process, full gap suite, two arms per
test (shadow-stack control + RS4GC), 479/479:

```
pass -> pass              447
diff -> diff               19   pre-existing, unchanged by the backend
node_fail -> node_fail     13   oracle cannot run the test
────────────────────────────
NEW REGRESSIONS             0
RS4GC refusals              0
RS4GC compile failures      0
```

Zero refusals is the load-bearing number, not zero regressions: **128 of the 479
tests contain `try {}`**, and the bridge cannot compile any of them. RS4GC
compiled every test in the suite.

**⇒ On aarch64, RS4GC-in-process is now a viable default.** Two things still gate
flipping it globally, and neither is correctness:

1. **`llvm-inprocess` is a non-default cargo feature.** RS4GC-as-default requires
   layer 0's feature becoming default first (#7301's scope, not this work's).
2. **x86-64 remains blocked on #7333** — see the measured `_Unwind_GetGR(ctx, 7)`
   segfault above. A default that only works on one architecture is not a
   default.

So the honest state is: *aarch64-viable, globally blocked on two pieces of scope
that are both already identified.*

### The adoption decision itself

Two precise-root mechanisms now exist. The kill-policy says that state is
temporary by design: **a mode that still exists is a decision that hasn't been
made.** Flipping the default to statepoints buys a bug class becoming
*unrepresentable* rather than *tested for*, and costs a knob surface plus parity
(not a win) on size. That decision is the plan's next real fork, and it is the
owner's — but it should be made on a schedule, not left to drift, because the
losing mode should stop compiling rather than linger untested.

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
| `matrix_multiply` | 631 ms | 32 ms | **✅ 70 ms shipped (9.9×) — #7296** | #7286 |
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
