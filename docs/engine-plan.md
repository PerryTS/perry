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
| 1 | `perry-codegen` lowering code | #7192, #7206, #7211 | `Raw`/`Rooted` borrow discipline | design **validated** (#7459); migration **started** (#7461); one bug shape **eliminated** crate-wide (#7462–#7465) |
| 2 | emitted code's liveness | #7280, #7271, #7252, #7243 | statepoints (#7108, #7174) | ✅ **THE DEFAULT** (#7370); landed #7314, made usable by #7339/#7340 |
| 3 | `perry-runtime` hand-written Rust | #7249, #7239, #7226, #7231 | `RuntimeHandleScope`, non-optional | still optional *inside* 107 listed modules; **595 modules locked at zero** by per-module ceilings (#7457), debt 1008 → 999 |

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

## ★ Status 2026-08-03 — the architecture is proven (ADOPTED 2026-08-04, #7370)

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

### ★ Update 2026-08-04 — both adoption gates are closed; statepoints run everywhere

The section above closes with *"aarch64-viable, globally blocked on two pieces
of scope."* Both pieces are now done, so that sentence should not be carried
forward either.

**1. x86-64 is no longer blocked (#7333 → #7349).** The `_Unwind_GetGR(ctx, 7)`
segfault is real and unfixable as stated — libgcc tracks only the columns CFI
restores, and RSP is derived from the CFA rather than tracked. The fix was to
stop asking for it: #7349 derives the SP-relative base from `_Unwind_GetCFA`,
which does work, with a per-architecture return-address adjustment (x86-64's
`call` pushes a return address, aarch64's `bl` does not — 8 bytes vs 0).
x86-64 Linux is a first-class arm of `gc-native-roots`, not a pinned refusal;
`statepoints-refuse-x86` is deleted along with the job that hosted it.

**2. Windows works (#7354 → #7355).** `RtlVirtualUnwind` steps a `CONTEXT`
outward and yields `Rip`/`Rsp`/`Rbp` directly, so the CFA derivation above is
not needed there. It is the one walker with no Itanium unwinder beneath it.

**Platform status, measured rather than assumed:**

| shape | map | walker | state |
|---|---|---|---|
| aarch64 + Mach-O (macOS/iOS/iPadOS/tvOS) | `__PERRY_GCMAP` | x29 chain, unwinder fallback | ✅ CI arm |
| x86-64 + ELF | `.perry_gcmap` | unwinder + CFA-derived SP | ✅ CI arm |
| x86-64 + PE | `.pgcmap` | `RtlVirtualUnwind` | ✅ CI arm |
| aarch64 + ELF | `.perry_gcmap` | x29 chain | ✅ CI arm (#7360) |
| watchOS / visionOS | ready | ready | compiler-side ✅; see below |
| ARM64 Windows | refused | none | open |

watchOS and visionOS are **not** blocked by Perry. `cargo check -p perry-runtime`
succeeds on stable for both with any feature set excluding `dyn-eval`; with it,
they fail three crates away in `psm`, whose Mach-O guard enumerates
`darwin/macos/ios/tvos` and omits `watchos`/`visionos`, so both fall to the ELF
branch and emit `.type`/`.size`. Verified by patching that one line: both then
build with full default features. They regressed on 2026-07-18 when `dyn-eval`
joined `default` (#6584) — nothing about the platforms changed. #7364 pins the
whole Apple target set compiler-side.

**One mechanism, not two.** `PERRY_STATEPOINTS` is deleted and the plain-map
bridge with it; `PERRY_RS4GC` is the only spelling, and the last stale references
went in #7362. The kill-policy line above — *"a mode that still exists is a
decision that hasn't been made"* — no longer applies to this pair, because the
losing mode stopped compiling.

**What the gate now proves.** Until 2026-08-04 the Unix arms reported
`frames_visited: 7, locations_visited: 0` — they would have passed with a walker
that visited nothing, since other root sources covered the probes. Windows
walked deep only by accident of heap sizing. `11_collect_at_depth` collects at
maximum recursion depth with one live root per frame (macOS 228/221, x86-64
Linux 231/221, both byte-matching the oracle), so `--require-locations` now
gates every arm (#7359).

**⇒ The remaining gate on adoption is `llvm-inprocess` becoming a default cargo
feature**, since RS4GC is the only invoke-capable backend. That is #7301's
scope and is in flight. Correctness and platform scope are no longer the
blockers; sequencing step 2 below (root density) is, because adopting today
would regress binary size on root-dense code.

### ★ Binary size, measured 2026-08-04 — it is a ROOT-DENSITY problem, not a metadata one

The note above says *"closing that axis needs **fewer roots**, not a tighter
encoding."* That is now measured, and the shape is sharper than "a wash".

Two synthetic programs, 2000 functions each, aarch64, `PERRY_STATEPOINTS=1` vs
the shadow-stack default:

| workload | total delta | `__text` | `__perry_gcmap` |
|---|---:|---:|---:|
| 2000 **root-free** functions (scalar only) | **+0 B** | +12 B | not emitted |
| 2000 **root-dense** functions (3 heap values live across an alloc) | +4,330,592 B (+18.95%) | +4,203,608 B | 902,124 B |

**The operative number is +1.86%, not +18.95%.** Measured on a real dependency —
`zod` from source, 81 native modules, a 29 MB binary — RS4GC in-process vs the
shadow-stack default:

| section | shadow | RS4GC | delta |
|---|---:|---:|---:|
| total | 28,955,656 | 29,495,048 | **+539,392 (+1.86%)** |
| `__text` | 20,989,544 | 21,508,556 | +519,012 |
| `__perry_gcmap` | 0 | 362,487 | new section |

**Do not quote the +18.95%.** It is a worst case constructed to isolate the
mechanism — three heap values live across an allocation in *every* function —
and it overstates real exposure by an order of magnitude. An earlier revision of
this section led with it and concluded root density was a prerequisite for
adoption; the dependency-scale measurement says otherwise, and adoption shipped
in #7370 without it.

Two things follow, and both matter for planning:

1. **Statepoints have no fixed cost.** A function with nothing live across a
   safepoint pays nothing at all — no map entry, no text. So the axis is not
   "statepoints are bigger", it is "roots are bigger", and a program's exposure
   is exactly its root density.
2. **97% of the growth is `__text`, not metadata.** #7314's compact map answered
   the metadata objection completely (it is 21% of the cost at this scale), but
   metadata was never the dominant term for root-dense code. The cost is the
   relocation sequence emitted per live root per safepoint.

⇒ **Do not spend further effort on the encoding.** The lever is safepoint density
and root-set size — which is the same lever #7287/#7296 are already pulling for
speed, so the two axes are aligned rather than in tension.

Runtime, same probes, quiet host, median of 5: statepoints are **1–2% faster**
across the board (2054 ms → 2013 ms total; every probe neutral or faster, none
slower), consistent with the −0.93% recorded above.

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

## ★ Status 2026-08-04, later — ELF was never compiling, and three CI arms were never running

A day of Layer 2 and Layer 3 work. The headline is not any single fix: it is that
two of the things this plan treated as *measured* were not.

### Statepoints could not compile on aarch64-ELF at all

The default is target-aware and aarch64-Linux is inside the allowed set, so this
was a hard compile failure on a default-on path — not a CI annoyance. Two
independent bugs, stacked, each masking the next:

1. **The compact stack-map parser did not model GNU-as symbol assignments.**
   `sym = expr` — the bare spelling of `.set`, zero bytes, no leading directive —
   so the dispatch reported the *symbol* as an unrecognised directive and refused
   the module. Emitted only at `-O3` (the optimiser materialises absolute-symbol
   aliases such as `perry_null_guard_zero = 0` and `.Lperry_ic_8 = .Ltmp3-4`) and
   only on ELF; Mach-O's asm printer does not use that spelling. #7390.
2. **The assembler was not told the CPU the code generator was told.** Perry
   compiles with `-mcpu=native`; on a Graviton runner LLVM emits SVE
   (`mov z1.d, #…`) and `compact_and_assemble` handed that text to clang with no
   `-mcpu` at all, so the assembler applied the portable baseline and rejected
   what the generator had just produced. #7390 forwards `-mcpu`/`-march`/`-mtune`.

Toolchain provisioning was a third layer: `setup-llvm22` installed `llvm-22-dev`
without `clang-22`, leaving `/usr/lib/llvm-22/bin/opt` with no clang beside it
(#7388), and the workflow then re-discovered a toolchain by hand and could land
on the distro's LLVM 18 — a green gate on the wrong LLVM. #7384 deleted the
hand-discovery in favour of `$LLVM_SYS_221_PREFIX/bin`.

**Reproduced without a Linux host**, which is the transferable part: the parser is
a pure function over assembly *text*, so what was needed was ELF text, not an ELF
machine. Trace the module, retarget the triple, drop the Mach-O-only
`.no_dead_strip` module asm, `opt -passes=rewrite-statepoints-for-gc`, then
`llc -mattr=+jsconv,+v8.3a` (generic ARMv8.0 cannot select `fjcvtzs`). The real
assembly then parsed at `-O2` and refused at `-O3` exactly as CI reported.

With both fixed the arm compiles everything and runs probe 1 with real GC
metrics, then segfaults in `02_survivor_promotion` under forced evacuation —
**#7392**, a genuine statepoint-lowering GC bug rather than a toolchain gap.

### ★★★ Three of the four RS4GC arms had never executed — not once

`gc-native-roots.yml` had **no `concurrency` group**, so nothing superseded a
stale run and its four-arm matrix multiplied on every push. Ten consecutive runs
were checked: `macos-14` was `queued` in **all ten**, as were `ubuntu-latest`
(x86-64 ELF) and `windows-latest` (PE). Only the aarch64 arm ever reached a
runner — which is precisely why it was the only arm ever observed red or green.

Fixed in #7393 with the same shape `llvm-inprocess.yml` already used (#7357):
push runs keyed on SHA, `cancel-in-progress` scoped to `pull_request`.

**This invalidates conclusions, not just tidiness.** Every "the ELF arm is the
only one red" statement rested on arms that had never run. It also makes #7392
unanswerable until they do: *"the segfault is ELF-specific"* and *"macOS has
never run the probe"* are indistinguishable.

Add to CLAUDE.md's four ways a gate cannot fail: **a matrix arm that never
reaches a runner presents as platform coverage while reporting nothing.**

### Layer 3 — nine fixes, and the finding that generalises

#7373 #7374 #7375 #7376 #7380 #7381 #7383 #7385 #7391. **Every one was an
ordering bug, not a missing root.** Each site already had rooting; what was
missing was ordering the root relative to the collection point. The rule is
*root before the allocation, re-read after* — not *add roots*.

Two consequences worth carrying:

- **A fault that MOVES is a real fix; a fault that does not move by a single
  byte means the value was already dead when you rooted it** — walk upstream, do
  not refine the root. This single test separated every real fix from every dud;
  ten attempts measured exactly zero and three of them shared the unmoved-fault
  signature.
- **Some catches are chains.** `js_object_get_field_by_name`'s `.size` arm had
  three stale-receiver sites masking one another (+664 → +560 → +820), and
  `gc_assign_string_source_rooting` likewise. "N remaining catches" undercounts.

One was not a rooting bug at all: #7380, a **type confusion**. The exotic-source
skip in `js_object_assign_one` classifies by GC type, and a RegExp is literally
`gc_malloc(GC_TYPE_OBJECT)` — so it passed the very test that excludes Map/Set/
Date and its `RegExpHeader` was read at `ObjectHeader`'s field offsets.
Generalises: **any "is this a plain object" test written as
`gc_type == GC_TYPE_OBJECT` is wrong for RegExp.**

#7391 is the one to remember for why the quarantine earns its keep: without it
the stale read is *silent*. The `CLOSURE_MAGIC` check simply fails, the
user-prototype link is skipped, and `foo.prototype = new Array(1,2,3)` quietly
does not take effect. Evacuation copies rather than zeroes, so a stale address
still holds plausible bytes and the program prints a wrong answer with no crash.

### ★ Layer 1 is further along than this plan assumed

`expr/temp_root.rs::lower_exprs_rooted` **already implements what the RFC
proposes for codegen operands**: lower left to right, root each finished value
across the evaluation of those that follow, gated on
`any_later_ref_may_trigger_gc` so an operand with nothing allocating after it
emits exactly the IR it did before. All four arms of `lower_call/func_ref.rs`
use it or its rest-bundling sibling.

So the codegen half of "rooting by construction" is substantially built, and
#7378's scoring says where the gap actually is: of four bugs found *after* the
RFC was written, it would have caught **one** — and the other three were layer 3,
where `RuntimeHandleScope` exists (675 uses / 169 files) but was **optional**.
#7389 is the first structural answer there: `RuntimeHandle::across_*` runs the
allocating call and returns the post-collection address in one step, so the
pre-call pointer is never bound, plus a **1006-site debt ratchet** wired into
`test.yml`.

**It is a debt counter, not a soundness proof, and says so.** Rust has no effect
system to mark "may allocate", so no signature can reject holding a stale copy
across such a call, and a `&mut Heap` token cannot cross `extern "C"`.

### RSS

#7377: nursery cap unconditional and scavenge default-on — **peak RSS −69%**.
Item 5 of the sequencing below is therefore partly answered already.

### Performance — first honest measurement, and two traps

- **Two top-level benchmarks measure nothing.** `bench_fibonacci` and
  `bench_bitwise` report `TOTAL:0` on Perry: the result is unused and LLVM
  eliminates the loop. Wall clock says ~240× faster than Node and *infinitely*
  faster respectively. #7395. The benchmark form of "the gate runs but its
  subject never did".
- **`bench_array_ops` is 4.5× behind Node** (262 vs 58, its own timer) and over
  half the time is outside generated code: `js_typed_feedback_numeric_array_
  index_set_guard` 103 samples, `js_array_grow` 87, `js_array_fill_f64_iota_
  extend` 89, GC temp-root ops 52. #7396.

  The guard is **not** overhead for a disabled feature — with typed feedback off
  it does the real work and authorises the fast store. The problem is siting: a
  five-argument cross-crate call per element for a predicate whose hot case is
  one masked load of the GC header's layout flag. That is the "native-able
  primitive became a runtime call" pattern in its clearest form.

A first fix attempt (slice-fill for the growth hole-init) was **reverted
unmeasured**: the disassembly check was vacuous (the symbol is absent from a
stripped binary, so `grep -c` returned 0 both before and after) and host load
had reached 55. Neither axis carried evidence. Re-measure on a quiet host with
`--debug-symbols` before trusting any array-path number.

### ★ Speed, resolved further the same night

The section above recorded the first measurement. Four corrections followed, and
the corrections are the useful part.

**The benchmarks were lying in two different ways** (#7403, shipped). Discarding
the result let Perry eliminate the loop; and even with the result consumed,
`fibonacci(FIB_N)` is loop-invariant, so it hoists out and runs **once** — the
checksum stays correct while `TOTAL` drops to 0. Both files now accumulate into a
printed `CHECKSUM:` and assert their subject was live. Corrected picture:

| benchmark | was reported | actually |
|---|---|---|
| `bench_fibonacci` | ~240x faster | **2.5x faster** |
| `bench_bitwise` | infinitely faster | **20.4x SLOWER** |
| `bench_array_ops` | (valid) | 4.2x slower |

**`bench_bitwise` is the biggest gap, and the fast path already exists.** The
hot loop contains **no runtime helper calls at all** — it is 4 `frem` per
iteration, and `frem` is not an aarch64 instruction: `4 x bl _fmod`, 1754 profile
samples. But `expr/binary.rs:588` already emits guarded `srem` for `%`, complete
with the IEEE `-0` correction, gated on `type_analysis::is_integer_valued_expr`.

The gap is the **analysis**, reduced to a one-line repro (#7404):

```ts
let a = 12345678; … a % 1000            // srem — fires
let a = 12345678; … a % 1000; a = a + 1 // frem — lost
```

Reassignment disqualifies the local. The obligation is judged in
`ProvenanceJudge::walk_expr` (`collectors/integer_locals.rs:632`).

**Four hypotheses about that subsystem were wrong**, and are recorded in #7404 so
they are not retried: that the mechanism lived only in `loops.rs` scoped to
counters; that a guarded `srem` needed building; that the fixpoint was
least-ordered; that no rule existed for reassignment. The module carries a
written exactness proof (*"judging once against the optimistic set and pruning
transitively is exact"*), so widening it is a proof-preserving change, not an
additive one — and a wrong widening returns silently wrong integer arithmetic
that `===` tests cannot see. Pair any attempt with the `Object.is` differential
table in #7404 and `bench_bitwise`'s `CHECKSUM:525000000`.

**A negative result on the array growth path** (#7396): `js_array_grow`'s
HOLE-init loop is **already vectorised** — 6 `stp` in the baseline. The proposal
to rewrite it as a `slice::fill` measures nothing. The original "zero paired
stores" reading came from grepping a symbol absent from a stripped binary, which
returns 0 whether or not the pattern exists. If there is a win in that path it is
the *pre-sizing policy*, not the cost of each grow. The store-guard half (103
samples) is untouched and remains the real target.

### Method notes that cost time to learn

- **Minimise the right statement.** `object_assign_collection` cost eleven wrong
  hypotheses because the program died four statements before the one the file's
  tail suggested — and the "minimal repro" built from it exited 0 the whole time,
  unchecked.
- **Verify a check can fail.** Three times in one day a check of mine could not:
  `PERRY_GC_HEAP_LIMIT=64` ran zero collections; a vectorisation check matched a
  symbol absent from a stripped binary; and a `grep -c` returning 0 was read as
  evidence when it meant "pattern never matched". A disassembly check needs a
  `--debug-symbols` build, an address resolved via `nm`, and an assertion that
  the extraction is non-empty.
- **A fix that compiles, passes every correctness test, and changes no emitted
  instruction is a fix to the wrong site.** The guarded `srem` written for #7404
  passed the whole `-0` edge-case table and emitted **zero** `srem`, because the
  gate never matched the path in question. Check the IR before the timer.
- **Baseline before attributing.** Two gap tests that failed alongside a patch
  turned out to fail identically on pristine `main`; the `perry-runtime` unit
  suite fails 3/5/3 across three runs of pristine `main` (#7365), so a raw
  failure count says nothing without it.

---

## ★★★ Status 2026-08-05 — four gates were dead, and that came first

**Before any of the layer work below was verifiable, four gates on `main` could
not fail.** Three were *required* contexts, so every merge in their windows
bypassed them. None surfaced from CI; each came from running the gate by hand
and asking whether its subject was actually present.

| gate | why it could not fail | fix |
|---|---|---|
| `gc-root-dominance` (curated corpus) | statepoints became the default lowering (#7370) and express roots as `gc.statepoint` bundles, not `@js_shadow_slot_bind` calls — the corpus held **0** of the checker's subject | #7452 |
| the same, **dependency-scale** corpus | identical cause; #7452 fixed one corpus and missed its twin. 81 modules, **0** bind call sites | #7460 |
| raw-handle debt ratchet | red since #7424, and its baseline only moves *down*, so it could not be satisfied by re-pinning | #7455 |
| the dominance checker itself | `ALLOC_RE` did not recognise `js_url_coerce_string`, so it could not see the #7453 bug class at all | #7454 |

**The lesson is the checkers' own design, not the incidents.** `gc_root_dominance_check.py`
refuses a clean verdict when it counts zero root stores — that guard is the only
reason the statepoint-era vacuity was visible at all. Every gate added here
should assert its subject was live, and CI's floors should be written so a
corpus that stops containing the subject goes red rather than green.

### Layer 1 — what changed

The RFC's proposed API **does not compile as written**: `emit_call(...).root(&mut e, …)`
is `E0499`, because the returned handle already borrows the emitter. Corrected in
#7459, where the design is also turned into `compile_fail` doctests pinned to
`E0499` and sabotage-tested, so the claim "the borrow checker rejects the bug
shape" is executable rather than asserted.

`FnCtx` has no interior mutability, so the borrow-carrying form cannot be built
on the real emitter. The shape that works there is the **combinator** — the same
one layer 3 settled on with `RuntimeHandle::across_*`: never hand out an unrooted
handle. That is what makes the migration incremental.

**One bug shape is now eliminated crate-wide**: a raw pointer bound before a
`lower_expr` and used after it. 11 sites in `url_main.rs` (nine URL setters,
the URLSearchParams family, `forEach`) and 3 in `child_proc.rs`. An arm-aware
scan now reports zero, with the single remaining hit confirmed *not* a bug — a
`perry/ui` widget handle, a registry id below the handle band.

**Four of those seven merges corrected the previous one.** Every defect compiled
cleanly with no warnings: a guard bound to `_operand_guard` that emitted no
truncate, a release placed inside one branch of an `if/else`, a fix that covered
the two-operand path and missed the three-operand one. Two of my scan counts
were also wrong — one high (arm-boundary false positives), one low (missed the
third operand). **This class does not survive review, only mechanical checking**:
read the emitted IR, audit reachability with a script, verify the runtime
semantics of anything that looks like a leak.

### Layer 3 — the ratchet was the wrong shape

A single global total cannot make the discipline non-optional: it is blind to
debt moving between modules, it lets a new file start dirty if something else
got cleaner, and it has no finish line. Since **595 of 705 runtime modules were
already clean**, #7457 inverts it — list the 110 modules *permitted* to carry
debt, and everything else must be zero. A cleaned module's line must be
**deleted** (an entry matching nothing fails), so cleanup is permanent and the
list can only shrink.

## Sequencing

**Updated 2026-08-04.** Step 2 below is complete: layer 0 landed (#7301), layer 2
landed (#7314) and became *reachable* (#7339) and *selectable* (#7340). The spine
`0 → 2` is done, so the ordering that remains is:

1. ~~**Next:** in-process LLVM (#7241) → statepoints (#7108/#7174).~~ **Done.**
2. **Reduce root density — worth doing, NOT a prerequisite.** Measured at
   dependency scale, statepoints cost **+1.86% binary size** against **−1–2%
   runtime**: a trade worth taking, and adoption shipped in #7370 without this.
   Statepoints carry **zero fixed cost** — a function with nothing live across a
   safepoint pays nothing at all — so a program's exposure is exactly its root
   density, and 97% of the growth is `__text` (the per-root relocation
   sequence), not metadata. #7314's compact map closed the metadata objection
   completely; metadata was never the dominant term. Fewer roots is still the
   same lever #7296 proved worth 9.9× on `matmul`, so size and speed pull
   together rather than trading off.
3. **Layers 1 and 3, independent of the above.** *Updated 2026-08-04.*

   **Layer 1 is largely built for codegen** — `lower_exprs_rooted` is the RFC's
   proposal, already gated on `any_later_ref_may_trigger_gc`, and all four arms
   of `lower_call/func_ref.rs` use it. The remaining Layer 1 question is not
   "build the mechanism" but "where is it still not used".

   **Layer 3 is where the gap is.** #7378 scored the RFC against four bugs found
   after it was written: one caught, three in `perry-runtime` where the mechanism
   did not exist. #7389 supplies the first half — `RuntimeHandle::across_*` plus
   a 1006-site debt ratchet — and the count is the worklist.

   Nine catches closed (#7373–#7376, #7380, #7381, #7383, #7385, #7391); the rest
   are **#7394**, whose documented cause is already fixed, so start from the
   disassembly rather than the tests' headers. Some catches are chains: one
   function held three stale sites masking one another.
4. ~~**Then the adoption fork.**~~ **CLOSED — statepoints are the default as of
   #7370.** Every gate it listed is shut: `llvm-inprocess` became a default
   cargo feature (#7353), x86-64 landed (#7349), Windows landed (#7355), the
   bridge was deleted leaving one backend (#7348), and the full 479-test gap
   suite with no env set matches the shadow baseline exactly — 447 pass / 19
   diff / 13 node_fail, zero regressions, zero compile failures, all 128
   try-carrying tests included.

   The default is **target-aware**, which is the part worth carrying forward:
   native roots where the runtime can walk the frames, shadow stack where it
   cannot. `gc_map` refuses to emit a map for a target whose bases the runtime
   cannot resolve, so a blanket flip would hard-fail every watchOS `arm64_32`
   and ARM64-Windows compile. Falling back is not "no roots" — it is the other
   lowering of the same analysis, which #7340 split apart precisely so this
   choice could be per-target.
5. **After the collector is trustworthy:** re-derive the RSS numbers (#7056).
   *Partly answered:* #7377 made the nursery cap unconditional and scavenge
   default-on for **peak RSS −69%**. What remains is the re-derivation under the
   statepoint default rather than the shadow stack.

7. **Performance now has a measured starting point, and two traps in front of
   it.** `bench_fibonacci`/`bench_bitwise` measure nothing (#7395) — fix or
   retire them before any performance claim cites them. `bench_array_ops` is
   4.5× behind Node with the array-store guard and growth path dominating
   (#7396). **Measure on a quiet host**: a first fix attempt was reverted because
   host load hit 55 and its disassembly check matched an absent symbol.

8. **CI hygiene is a correctness input, not housekeeping.** Three of four RS4GC
   arms had never executed (#7393). Before citing any matrix as platform
   coverage, confirm its arms actually reach a runner.
6. **Do not** re-measure GC pacing, or update the README's performance table,
   mid-cycle.
