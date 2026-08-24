# Explicit statepoint GC experiment

> **Historical experiment journal.** Its sections retain the decision at each
> measurement date, including conclusions superseded later in this same file.
> Native RS4GC roots now ship by default on supported 64-bit AArch64/arm64 and
> x86-64 targets; other targets use shadow frames. See
> [the current collector page](src/internals/garbage-collector.md#roots-by-target).

Date: 2026-07-31

Branch: `exp/stackmap-viability`

Base commit: `e2557c1a985cb983ed00aafd1a2c1b31f1570b98`

## Decision

The explicit `gc.statepoint` bridge is correct enough to validate the
mechanism, but it is not currently a performance win and does not yet make
Perry's GC simpler. Keep the shadow stack as the default.

- All eight GC-ratchet probes pass normally and with forced evacuation plus
  evacuation verification.
- The full suite emits 1,080 statepoints and 1,562 relocations. It has zero
  plain-stack-map fallbacks at GC-relevant calls.
- Runtime is effectively flat versus the shadow stack: -0.27% geometric mean
  on a heavily loaded host. It is 1.42% slower than the plain-stack-map arm.
- Uncached compilation is 2.12% slower than shadow-stack compilation.
- Statepoint stack-map payload is 2.01x the plain-map payload across the suite.
- Relocation is better expressed: LLVM now owns the call/result/relocated-value
  relationship, and the compiler memory barriers from the plain-map prototype
  disappear.
- The whole system remains more complex because native unwinding, stack-map
  parsing, textual call rewriting, root liveness, fallbacks, and
  platform-specific metadata retention are still required.

The prototype remains opt-in with `PERRY_STATEPOINTS=1`. The default
shadow-stack path is unchanged.

## Follow-up: root pressure and audited safepoints

The first prototype treated almost every textual call with live roots as a
safepoint. That was correct but needlessly pessimistic. The follow-up adds an
audited GC-call-effect table whose only claim is whether a helper can enter
Perry's collector. Unknown calls remain safepoints.

This is intentionally separate from LLVM memory effects. Temporary-root
bookkeeping, write barriers, layout notes, feedback counters, and refcount
writes mutate memory, but they do not run a Perry collection and therefore do
not need stack-map metadata.

`--statepoint-report[=json]` makes the resulting root pressure visible. It
reports per-function logical/bound root slots, calls with live roots, audited
non-collecting calls, statepoints, relocations, plain-map fallbacks, live-root
widths, and callee frequencies. It is observational and disables cache reuse
for the reporting run:

```sh
PERRY_STATEPOINTS=1 perry compile app.ts --statepoint-report
```

On `benchmarks/app-patterns/kernels/batch.ts`, the audit changed:

| Metric | Before | After | Change |
|---|---:|---:|---:|
| Statepoints | 442 | 219 | -50.5% |
| Relocations | 867 | 403 | -53.5% |
| `__llvm_stackmaps` | 54,968 B | 26,432 B | -51.9% |
| Plain-map fallbacks | 0 | 0 | unchanged |

The report found 223 calls with live roots that cannot collect. The largest
groups were typed-feedback bookkeeping, class-field guards, temporary-root
push/get/truncate, layout notes, write barriers, and property-observation
records.

Across the eight GC probes:

| Probe | Statepoints before | Statepoints after | Relocations after | Calls skipped |
|---|---:|---:|---:|---:|
| Nursery churn | 152 | 65 | 91 | 88 |
| Survivor promotion | 165 | 79 | 132 | 88 |
| Cross-generation writes | 168 | 74 | 95 | 96 |
| Dead after deep stack | 119 | 55 | 59 | 65 |
| Closure capture | 146 | 69 | 81 | 78 |
| String retention | 92 | 45 | 46 | 50 |
| Array grow/evacuate | 100 | 49 | 49 | 52 |
| Map/set side tables | 138 | 66 | 124 | 74 |
| **Total** | **1,080** | **502** | **677** | **591** |

The same audit applies to the plain-map backend. Its eight-probe metadata
payload is now 26,816 bytes; explicit statepoints use 53,952 bytes. Statepoint
metadata therefore remains 2.01x plain maps even after both shrink by roughly
half. Reducing the number of maybe-pointer roots through representation
promotion remains the larger shared lever.

All eight probes pass in shadow, plain-map, and statepoint modes with forced
evacuation and relocation verification: 24/24 mode/probe comparisons match
Node's probe/checksum output.

### Follow-up runtime and compile measurements

Each runtime cell below is the median of seven executions on the same host and
full-feature runtime artifact. Host variance was high, so the numbers are
directional:

| Probe | Shadow | Plain stack map | Statepoint | Statepoint vs plain |
|---|---:|---:|---:|---:|
| Nursery churn | 196.698 ms | 194.860 ms | 195.178 ms | +0.16% |
| Survivor promotion | 234.184 ms | 235.600 ms | 233.294 ms | -0.98% |
| Cross-generation writes | 383.002 ms | 330.386 ms | 341.206 ms | +3.27% |
| Dead after deep stack | 1,029.803 ms | 996.121 ms | 1,196.806 ms | +20.15% |
| Closure capture | 230.904 ms | 212.415 ms | 188.984 ms | -11.03% |
| String retention | 151.553 ms | 173.651 ms | 162.272 ms | -6.55% |
| Array grow/evacuate | 251.063 ms | 267.094 ms | 237.260 ms | -11.17% |
| Map/set side tables | 577.092 ms | 563.401 ms | 601.368 ms | +6.74% |

Geometric means put plain maps at -1.17% versus shadow, statepoints at -1.54%
versus shadow, and statepoints at -0.38% versus plain maps. That small aggregate
statepoint lead is below the noise floor; the deep-stack regression remains a
clear negative signal.

For uncached `batch.ts` compilation, seven-run medians were 910.3 ms shadow,
946.0 ms plain maps (+3.92%), and 958.3 ms statepoints (+5.28%).

### Follow-up: x29-chain fast walker (2026-07-31, after rebase onto #7114)

The deep-stack telemetry below (36,458 frames unwound for 104 root
locations) identified `_Unwind_Backtrace` as the walker bottleneck: full
compact-unwind register recovery on every native frame. The branch now walks
the raw x29 chain instead — two loads per frame — enabled by two facts:

1. Generated functions now carry `"frame-pointer"="non-leaf"`. Textual-IR
   input gets no frame-pointer default from the clang driver, which is why
   generated code previously saved x29 without establishing a chain.
2. Statepoint spills are SP-relative (`Indirect [R#31 + N]`) on AArch64
   regardless of frame-pointer attributes, but the stack-map header records
   each function's frame size, and LLVM's AArch64 frame keeps the
   `[x29, x30]` pair at the top of the frame — so the body SP is always
   `fp + 16 - stack_size` from the same chain loads.

The fast path is fail-closed twice over. At parse time, any location that is
not FP-relative or sized-SP-relative marks the whole image not chain-walkable.
At walk time, a misaligned, non-increasing, or out-of-bounds frame pointer
abandons the walk and re-runs the platform unwinder (slot visits are
idempotent, so a partial fast walk followed by a full unwinder walk is safe).
`PERRY_STACKMAP_WALKER=unwind` forces the old walker as a bisection control;
`PERRY_STACKMAP_WALKER=verify` runs both and panics unless they visit the
identical slot set. Verify exists because forced-evacuation verification
enumerates roots through the same walker and therefore cannot catch a walker
that silently skips frames — this is CLAUDE.md gate-failure mode 4 applied to
the walker itself. Telemetry gains `fp_walks`/`fallback_walks` so any run can
prove which walker executed.

Results (loaded host, directional): 24/24 correctness matrix, 16/16
verify-mode probe runs (fast walk engaged and byte-identical to the unwinder
everywhere), and the deep-stack statepoint probe improves 4.8% end-to-end
against the unwinder walker on the same binary with interleaved reps.

A finding for the mode decision fell out of the register census: plain-map
mode emits `Register R#1` locations — the root slot's address materialized in
a caller-saved register at the map point. No parser can soundly use that
location (the register is clobbered by the callee and unrecoverable at GC
time), so those roots are structurally invisible to the collector. LLVM's
stackmap intrinsic offers no way to force the address into memory; statepoint
spill slots cannot exhibit the problem. Plain maps are therefore unsound by
construction at a small but nonzero rate (3 of 60 locations on the deep-stack
probe), which strengthens the case for deleting the plain-map arm once
statepoints match it on the walker-sensitive workloads.

### Native-root and unwinder telemetry

Native stack-map roots now have their own `root_sources.compiled_native`
telemetry bucket instead of being incorrectly charged to
`compiled_shadow`. `root_sources.native_stack_maps` also records walks, frames
visited, records matched, and locations visited.

The forced-evacuation deep-stack probe reported 105 walks, 36,458 frames
visited, 36,139 records matched, and only 104 root locations visited, with a
maximum of 694 frames in one cycle. The walker is therefore a justified
optimization target, but a direct frame-pointer walker is not yet a safe
substitution: current generated AArch64 code saves `x29` without consistently
establishing an `x29` frame chain, and Rust/runtime frames have no matching
contract. A fast path first needs an explicit frame-pointer/unwind ABI for
generated and intervening runtime frames, plus fallback and cross-architecture
tests.

### Follow-up: the explicit-safepoint collection contract (PERRY_GC_SAFEPOINT_ONLY)

The prerequisite that gated this experiment — the #7114 temp-root
correctness fix — landed on main during the first prototype session, so the
contract experiment ran after rebasing onto it.

**The contract.** A collection that skips the conservative stack scan
consumes only precise roots; with native stack maps active, precise frame
roots exist only at mapped PCs. Therefore such a collection may only begin
at a declared safepoint (a loop back-edge poll or the outermost
microtask-pump boundary) — anywhere else it must scan conservatively. The
runtime already routes moving minors to those safepoints (the #7024
deferral machinery), so today the property is *emergent*: it holds because
every possibly-collecting call happens to be mapped. The contract makes it
*enforced* — a thread-local declared-safepoint flag plus a check at the
root-scan subphase — and enforcement is what makes it sound to stop mapping
call sites.

Two enforcement levels: `PERRY_GC_SAFEPOINT_ONLY=1` (heal — an undeclared
precise-root cycle gets the conservative scan forced for that cycle, which
restores liveness and keeps it non-moving) and `=strict` (panic — the gate
mode that proves the enforcement is live, per the four-ways-a-gate-cannot-
fail rule). Manual `gc()` and the alloc-point slack valve force the scan
already and are exempt by construction. (An earlier revision also drained
non-nursery triggers at every allocating loop back-edge; that turned churn
loops into per-iteration collection work — O(n²) — and was deleted. The heal
alone is sufficient: undeclared full collections simply pay one conservative
scan.)

**What it unmaps.** A new audited `GcCallEffect::AllocNoReentry` class:
helpers that may allocate (arming a trigger) but never collect synchronously
and never re-enter generated JS. Under the contract their call sites need no
statepoint — any trigger they arm either defers to a declared safepoint or
collects behind the forced scan. First audited set: singleton closure
allocation, class-object allocation, `js_array_push_f64`, `js_array_length`,
`js_array_slice_values`.

**Measured results (loaded host, correctness-grade).** All gates green at
`4e3d5c70e`: 16/16 probe cells (forced evacuation + walker-verify under the
contract), strict-mode enforcement fired on the deliberately unsound
configuration (`PERRY_GC_SCAVENGE=1` + polls off — a precise-root minor at
an unmapped alloc point aborts with the contract panic), and max RSS is
unchanged by deferral (27/27 MB and 36/36 MB on the two churn-heaviest
probes). The audited five-helper set removes 7.8% of `batch.ts` statepoints
(217 → 200) and 7.5% of relocations. Getting the contract here found and
fixed three implementation bugs, each caught by a gate: enforcement that
missed the copying-minor path, a heal that overrode a local decision while
copying eligibility read the global one (real memory corruption under
forced evacuation), and a per-poll trigger drain that turned churn loops
into O(n²) collection work. All three fixes deleted code.

**The census result that bounds the idea.** On `batch.ts`, 217 statepoints
break down as roughly 85 property-access diamonds (getter re-entry possible
— must stay mapped), ~40 coercion/setter/throw paths (re-entry — stay), ~10
generated-to-generated calls and polls (stay by definition), and only ~25-30
pure-allocation sites the contract can unmap. **Re-entry, not allocation, is
what bounds the contract's reach on object-heavy code.** Deleting the
property-access calls is representation selection's job (`Ptr<Shape>`); the
contract unmaps what allocation traffic remains. The two campaigns compose
rather than compete.

### Work deliberately left gated

This follow-up does not alter temporary-root semantics, collection scheduling,
or the conservative native-stack fallback. The representation plan makes the
temp-root correctness work a prerequisite for an explicit-only collection
contract and conservative-scanner removal. Doing that here would overlap the
other agent's work and make failures impossible to attribute. Once that
prerequisite lands, the next experiment is to assert that moving collections
occur only at declared safepoints and then measure whether the conservative
scanner can be deleted.

## Quiet-host matrix (2026-08-01, reserved Mac mini)

First measurement of this experiment not taken on a loaded host: Apple M1
(4P+4E), macOS 26.5.1, the gc-ratchet pinned-baseline platform, reserved
with baseline load 1.4–1.9 from release infrastructure only (recorded
per-rep). Artifacts shipped SHA-pinned from `4e3d5c70e` (no cargo on the
host); all four arms hash-distinct per probe; 8×4 forced-evacuation preflight
plus walker-verify and the strict-enforcement gate all green there before
any timing. 11 interleaved reps, rotated arm order, `/usr/bin/time -l`.
Caveat: 10 ms timer granularity puts ±1 quantum (≈2–9% on these probe
durations) on any single cell; medians were stable across reps.

Runtime, median seconds (spread), delta vs shadow:

| Probe | Shadow | Plain map | Statepoint | Contract |
|---|---:|---:|---:|---:|
| Nursery churn | 0.160 | 0.160 (+0.0%) | 0.160 (+0.0%) | 0.160 (+0.0%) |
| Survivor promotion | 0.190 | 0.180 (−5.3%) | 0.190 (+0.0%) | 0.190 (+0.0%) |
| Cross-gen writes | 0.190 | 0.180 (−5.3%) | 0.180 (−5.3%) | 0.190 (+0.0%) |
| **Dead after deep stack** | 0.430 | 0.410 (−4.7%) | **0.410 (−4.7%)** | 0.410 (−4.7%) |
| Closure capture | 0.140 | 0.130 (−7.1%) | 0.130 (−7.1%) | 0.130 (−7.1%) |
| String retention | 0.110 | 0.110 (+0.0%) | 0.120 (+9.1%, one quantum) | 0.120 |
| Array grow/evacuate | 0.150 | 0.150 (+0.0%) | 0.150 (+0.0%) | 0.150 (+0.0%) |
| Map/set side tables | 0.430 | 0.430 (+0.0%) | 0.430 (+0.0%) | 0.430 (+0.0%) |

Geometric means vs shadow: plain maps −2.83%, statepoints −1.10%,
contract −0.43%.

**The deep-stack weakness is closed.** The probe that was ~20% slower on
the loaded host is now 4.7% *faster* than shadow, and the attribution is
exact: the same statepoint binary with `PERRY_STACKMAP_WALKER=unwind` runs
at 0.430 — precise shadow parity — so the x29-chain walker is the entire
difference.

Max RSS: every cell within ±0.8% of shadow (ratchet-comparable platform).
Uncached `batch.ts` compile: shadow 0.590 s, statepoints 0.570 s (−3.4%) —
the loaded-host "+5.3% slower to compile" claim did not survive quiet
measurement and is withdrawn.

Metadata (`__llvm_stackmaps`, summed over the eight probes): plain maps
42,936 B, statepoints 81,104 B (1.89×), contract 73,952 B (−8.8% vs
statepoint). Generated `__text` is ~5.8 KB smaller across the eight
binaries in the native arms (probe code is small; the shadow-stack text
delta scales with generated code, per #7108's 13.3% on a real app).

Standing conclusion after this matrix: on wall-clock, RSS, and compile
time, statepoints are at worst tied with the shadow stack on this
hardware; metadata remains the only losing axis, and it is the axis
repsel promotion shrinks. Small-hardware and Linux numbers still require
the ELF scanner port.

## Post-matrix follow-through (2026-08-01, `897e0f53b`)

Two changes landed after the matrix, both gate-verified (16/16 forced
evacuation + walker-verify, strict-enforcement gate fires, RSS flat):

1. **The plain-map user mode is deleted** per the GC knob kill-policy:
   after the quiet matrix it was a losing configuration (statepoints match
   it within timer quantization), and it is structurally unsound — LLVM's
   stackmap intrinsic can record a root slot's address as `Register R#N`,
   caller-saved and unrecoverable at collection time, so those roots are
   invisible to the collector by construction. The lowering survives only
   as statepoint mode's internal `try`/setjmp fallback; shrinking that
   fallback set is the remaining correctness work for the backend.
2. **Noreturn call sites carry no metadata** (`GcCallEffect::NeverReturns`):
   every `js_throw*` helper funnels into `exception::js_throw` (`-> !`), so
   control never returns, no relocation is ever consumed, and the frame's
   roots are dead past the call. Sound in any mode; deeper frames carry
   their own records.

Metadata trajectory on `batch.ts` statepoints, all without any
representation-selection improvement: 442 (first prototype) → 217
(call-effect audit) → 198 (noreturn elision) → 181 (contract) → **172 after
the second audit round — −61% total**. The remaining big step is the property-access diamonds
(~85 sites), which fall to repsel `Ptr<Shape>`.

## The compact per-function experiment — a measured NEGATIVE result

The per-function precision model was then built and disproven
(implementation at `bd066d62b`, deleted from the tip afterward — an unsound
mode must not survive as a configuration a future bisect will trust).

**The thesis**: one entry stackmap per generated function recording every
root alloca as a stable Direct location; calls carry only memory barriers;
a `__perry_gen_end` sentinel object linked last bounds the generated region
so the runtime can match frames by region instead of per-safepoint PCs.
**The size result was real**: 424–680 B of metadata per probe binary versus
5.3–8.9 KB for statepoints — 10–13× — with `__text` mostly smaller too.

**The correctness result kills it.** A ten-line churn loop
(object escapes into a ring, two field reads) deterministically computes
wrong values. The forensics chain, recorded because each step eliminated a
plausible-but-wrong theory: retention-clear lowering (no effect),
callee-saved register clobbers at barriers (no effect, bit-identical
failure), dead-slot zeroing before every GC-capable call (no effect,
bit-identical), and finally disabling the walker's visits entirely —
**still bit-identical corruption**, proving the stack-map machinery was
never the vector. The corrupted fields contain forwarding-stub and
header-age-bit patterns: the mutator reads from-space through a stale
pointer that lives in optimized SSA, not in any root slot. The same module
compiles to **79 `gc.relocate`s** under the statepoint backend — each one a
place where LLVM held a heap-derived value whose post-collection identity
only relocation semantics can restore. `asm "~{memory}"` constrains memory
ordering, not dataflow; no barrier discipline reaches values the optimizer
carries in registers and rematerializes.

## Real-app remeasurement (test-drizzle-pg, 133 modules, 2026-08-01)

#7108's size model, re-taken as a direct measurement on the same
application with every in-branch reduction live:

| Arm | file | `__text` | `__llvm_stackmaps` |
|---|---:|---:|---:|
| shadow (default) | 28,474,576 | 20,376,748 | 0 |
| statepoint | 32,206,720 | 20,227,252 | 4,025,336 |
| statepoint + contract | 32,008,560 | 20,226,728 | 3,832,384 |
| + second audit round | 31,925,984 | 20,223,400 | **3,757,520** |

Two model corrections, one in each direction. The metadata came in at
**3.83 MB — below the refined model band's 4.5 MB floor** (the audit,
noreturn elision, and contract compose better on real dependency code than
the all-roots-live worst case assumed). But the text actually recovered is
**150 KB, not the 439 KB** #7108 reported — that figure measured
`PERRY_SHADOW_STACK=0` (rooting fully off) as the floor, while real
statepoint codegen keeps spill/reload work. Net file-size cost of the best
native arm on a real app: **+3.53 MB (+12.4%) versus shadow — a ~25×
imbalance that no audited elision closes.** The contract's real-app effect
is −4.8% metadata (probe-scale was −8.8%; dependency code has
proportionally fewer audited-helper sites).

**Verdict as of 2026-08-01 (superseded below):** the shadow stack was the
three-axis optimum — wall-clock tied within timer quantization, RSS tied,
file-size won by 3.5 MB on a real application. The statepoint backend was
correctness-superior (the forgot-to-root class is structurally impossible),
speed-competitive, and 59% leaner in metadata than its own first prototype,
but carried a 25× metadata imbalance that no audited elision closed.

That conclusion assumed the metadata's *content* was the cost. It was not.

## The file-size axis was the wire format, not the roots (2026-08-03)

Re-examined with `scripts/stackmap_anatomy.py`, which breaks the section
down by structural component and asserts it parsed 100% of the bytes.

**First correction: generated code is already smaller under statepoints.**
On `test-drizzle-pg` the RS4GC arm's `__text` is 20,128,708 against shadow's
20,376,748 — **248 KB better**. The entire loss is `__llvm_stackmaps`.

**Second correction: most of that section is provably dead weight.**

| component | share of section |
|---|---:|
| `Constant` location slots (3 per record: CC / Flags / NumDeopt) | 40.6% |
| duplicate base/derived location slots | 13.3% |
| record headers (incl. an 8-byte patchpoint ID never patched) | 18.0% |
| inter-record padding | 11.3% |

`stack_maps.rs` **already discarded the constants and collapsed the
base/derived pair at parse time**. Over half the section was shipped in the
binary and thrown away at startup — redundancy, not a tradeoff. LLVM's
stack map is a JIT-patching wire format; an AOT collector needs
`{dwarf_reg, offset}` per distinct root and nothing else.

**Compaction measured on drizzle** (4,214,384 B, 124 concatenated maps,
1,717 functions, 33,406 records, 154,020 distinct roots):

| encoding | bytes | ratio |
|---|---:|---:|
| flat varint (drop constants + duplicate pairs) | 387,199 | 10.9× |
| + roots sorted and delta-encoded | 286,258 | 14.7× |
| + "same live set as previous record" flag | 132,418 | 31.8× |
| **shipped**: as above, but offsets fixed-width | **224,832** | **18.7×** |

The third row is the big one and it is a fact about real programs, not a
coding trick: **77% of records have exactly the live set of the record
before them**, because consecutive safepoints in a function share their
roots. That same fact shrinks the in-memory index — the decoder points
repeats at one copy instead of materialising 154k entries — so it is an RSS
win as well as a file-size one.

The fourth row is what actually ships, and the difference is a constraint
rather than a choice: **at `-O3` LLVM emits each record's instruction offset
as a label difference** (`.long Ltmp9-_main`) that only the assembler can
evaluate, so those offsets cannot be delta-varint-encoded at rewrite time.
They go in a fixed-width `u32` array instead, costing ~4 bytes per record.
Recovering the last 92 KB would mean assembling twice — once to learn the
numbers the assembler just computed, once to emit them — which is more
machinery than the bytes are worth.

### Measured, not projected (2026-08-03)

Built with one compiler, identical flags, and a **clean object cache per
arm** (a clean-cache rebuild reproduced the cached shadow figure to within
8 bytes, so nothing here is a stale-artifact reading):

| arm | total | `__text` | `__perry_gcmap` | vs shadow |
|---|---:|---:|---:|---:|
| shadow (default) | 28,737,536 | 20,646,900 | 0 | — |
| statepoint + compact | 28,688,464 | 20,497,296 | 227,275 | **−49,072** |
| RS4GC + compact | 28,605,912 | 20,409,232 | 224,126 | **−131,624** |

The emitted map came in at 227,275 B against the 224,832 B the encoder model
predicted — within 1%. Metadata fell from 4,214,384 B to 227,275 B (18.5×),
and `__text` is 149,604 B smaller than shadow's on the same build.

**The file-size axis is flipped.** The statepoint backend now leads on
**all three axes** — wall-clock −0.93%, RSS flat, and size −131,624 B on the
RS4GC arm — where it previously lost size by 3.5 MB.

Both arms pass the full gate: 8/8 probes byte-match the pinned Node oracle
normally *and* under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1
PERRY_STACKMAP_WALKER=verify`, which is the check that can actually fail if
the new format decoded to a smaller root set — a map that lost roots would
corrupt the heap under forced evacuation rather than merely print something
different. The gate also asserts its subject was live: `__llvm_stackmaps`
absent **and** `__perry_gcmap` non-empty, before any output is compared.
Its first run correctly reported 0/8 because the rewrite had silently not
run at all.

**Compile-time cost, stated honestly:** 11.95s vs 10.43s for the whole
application (+14.6%), covering statepoint lowering plus the assembly round
trip. That is not one of the three axes being optimised, and it buys the
axis that was losing.

### Why the rewrite happens on assembly

`clang -S` prints the stack map as ordinary directives with the function
addresses as **symbol names in plain text** (`.quad _main`), so one text
parser replaces Mach-O *and* ELF relocation parsing, `llvm-objcopy`, and a
second link pass. Two facts settled this empirically rather than by taste:

- the stack map's function-address fields are **external symbol
  relocations** (`otool -r`: `extern 1` at offsets 16 and 40), so a
  separately assembled table can reference them by name and the linker
  resolves it — no relink needed;
- `-S` costs nothing: it takes the **same 0.04s as `-c`** (codegen is the
  cost, printing text is free), and `llvm-mc` assembles in 0.02s. Use
  `llvm-mc`, not `clang -c file.s` — the latter is 0.13s of driver overhead.

### Two traps that cost time here

- **On-disk stack-map addresses look like garbage** (`0x00300000000008b0`)
  because they are **dyld chained-fixup entries**, not addresses: bits 0-35
  are the target, bits 51-62 the chain delta to the next fixup. dyld
  resolves them at load. Do not conclude records are mismatching from an
  on-disk read.
- **The section is a concatenation of one map per object file**, not a
  single map. A parser that reads only the first header silently
  under-counts, and nothing downstream notices. Assert that parse coverage
  equals the section size.
- **`.no_dead_strip` names the block's label from outside the block.**
  Removing the label without retargeting that directive leaves an undefined
  symbol — and the directive is also the only thing keeping a section
  nothing references from being stripped, which would leave the collector
  with no roots at all.

### The transfer question, also measured: can the audit shrink the SHADOW stack?

The audited call-effect facts apply to shadow bookkeeping in principle — a
function whose every call is provably non-collecting needs no shadow frame
at all, by the same soundness argument the statepoint elisions passed gates
with. Census on the real app's traced IR (instrument validated against
#7108's function totals): **118 of 1,535 shadow-framed functions qualify
(7.7%), covering only 4.0% of shadow-op IR lines.** The elidable functions
are small leaves; the cost lives in large functions with genuine collecting
calls. Whole-frame elision would recover well under 1% of generated text —
recorded here as measured-and-not-pursued (finer per-region elision is
complexity the win does not justify). The shadow stack's remaining text
cost is, as the repsel campaign already measured from the other side,
bookkeeping for values that cannot yet be proven non-pointers — one more
place every road converges on representation selection.

## RS4GC pipeline slice (#7174) — running, fully gated, 2026-08-01

`PERRY_RS4GC=1` exists and passes the complete gate matrix: 8/8 probes
under forced evacuation + verification and 8/8 under walker-verify, with
`RewriteStatepointsForGC` inserting every statepoint, relocation, and
downstream-use rewrite over surgically-retyped `ptr addrspace(1)` root
SSA. Requirements established empirically:

- **Version-matched toolchain**: LLVM 22 `opt` output is unparseable by
  Apple clang 21 — `PERRY_LLVM_CLANG` must point at the same LLVM's clang.
- **Statepoint placement must precede cast-merging optimization.**
  `default<O2>` before RS4GC fails 3/8 probes: GVN/CSE merges the per-site
  `ptrtoint`/`bitcast` chains across future statepoint sites, recreating
  the stale-double hazard. `function(mem2reg)` alone is the sound
  pre-pass; clang's full optimization AFTER statepoint insertion is safe
  by construction. This is the same design law again, now stated
  positively: relocation semantics must be present before the optimizer
  is allowed to move heap-derived values.
- **Two vacuous-green runs preceded the real one**: the fail-closed
  surgery bail (recognizer knew only the unit-test `alloca i64` idiom;
  real roots are `alloca double`) silently routed every function to the
  explicit bridge. Caught by record-count comparison (200 vs 55), not by
  any probe — assert surgery liveness before believing an RS4GC A/B.

Probe-scale metadata trails the bridge (probe 01: 8,072 B vs 5,320 B —
the zero-live statepoint record constant dominates small functions), but
the real-app measurement flips the hierarchy: on `test-drizzle-pg`,
RS4GC `__text` is 20,128,708 — **248 KB below shadow and 98 KB below the
explicit bridge** — and metadata is 3,875,416 B, within 3.1% of the
audited bridge's 3,757,520. Total file: 31,957,792, the smallest native
arm measured. SSA liveness pruning compensates for the record constant at
scale exactly as predicted. Leaf-attribute transfer verified by record
count (200 → 103 on probe 01). Runtime (8 probes × 9 interleaved reps, loaded host, arms share load —
directional): shadow 218.1 ms geo-mean, bridge+contract 216.3 ms (−0.83%),
RS4GC 216.1 ms (−0.93%) — the fastest arm measured. Max RSS: flat with
shadow on the four churn-heaviest probes (27/27, 36/36, 37/38, 25/25 MB).
RS4GC is therefore already the preferred
native backend on every axis measured except probe-scale metadata, while
carrying the structural correctness model — the explicit bridge becomes a
deletion candidate once RS4GC grows has_try coverage and a leaner
zero-live-record story (upstream pass option recorded on #7174).

## The repsel-erasure projection, measured — and corrected

Both this campaign and the representation-selection plan share the
assumption that repsel promotion erases native-root metadata ("each value
proven non-pointer deletes its records"; the repsel plan itself notes "the
GC currency was not measured at all"). First measurement, using the #7133
knob-scoping fixes: `batch.ts` under statepoints with all landed
promotions on versus `PERRY_CANONICAL_{I32,U32,STR}_LOCALS=0` is
**byte-identical** — 24,752 B of metadata, 198 statepoints, 363
relocations, 33 root slots, unchanged.

The correction this forces: the landed promotion classes remove *calls
and guards* (the performance currency), but they promote values the
rooter's type analysis already classified non-pointer — so they delete
zero roots. The metadata-erasure currency is paid only by promotions in
the maybe-pointer population: untyped locals, temporaries, and dependency
JS — exactly where repsel coverage is currently weakest (the
`__esModule` barrier, minified slot reuse). The "repsel erases the 25×
gap" projection therefore needs either the Track E/F work (types made
load-bearing, dependency-JS recovery) or `Ptr<Shape>`-class promotions
feeding a typed-slot story, not wider scalar coverage. Recorded so
neither campaign builds on the uncorrected assumption.

## Linux verification and the small-hardware numbers (#7173, 2026-08-01)

Runtime verification on two granted Linux hosts, cross-built from macOS
(perry `--target linux`/`linux-aarch64` `--no-link` for ELF probe objects;
`cargo zigbuild` archives with `-Cforce-frame-pointers=yes`; `zig cc` link
with `-lunwind`):

- **x86-64 (Ubuntu, idle prod webserver): 8/8** forced-evacuation probes
  byte-matched to the pinned oracles, first run — first execution of the
  ELF section discovery and Linux unwinder path.
- **aarch64 (Raspberry Pi 5): 8/8 after one caught defect.** The
  verify-walker mode fired exactly as designed: the Darwin SP
  reconstruction (`SP = FP + 16 − stack_size`) is wrong on aarch64-Linux
  (frame pair at the bottom, not the top) — fast walk and unwinder
  disagreed by the layout delta on one slot. Fix: SP-relative locations
  disqualify the fast chain off-Darwin — and the follow-up disassembly
  proved this permanent, not provisional: generated prologues set
  `x29 = sp + 0x30` and `x29 = sp + 0x60` for stack sizes 128 and 192 —
  the offset varies per function with the callee-save area below the pair,
  so no `(FP, stack_size)` formula exists on aarch64-Linux. The unwinder
  is the sound Linux path; a Linux fast chain requires FP-relative spills
  from LLVM (upstream) or per-function side metadata (the compact-section
  ghost). A silent-fallback foot-gun was also found:
  an unrecognized `--target` value compiles for HOST (Mach-O out of
  `linux-arm64`); the accepted spelling is `linux-aarch64`.

**Small-hardware timing (Pi 5, load ≤0.1, 9 interleaved reps)**: shadow
469.2 ms geo-mean, statepoints 538.2 ms (**+14.7%**; deep-stack +23%,
string-retention +35%, array-grow +32%). The M1 parity does NOT transfer
to narrow cores.

**Decomposition — the delta is COLLECTOR-SIDE, and specifically the
unwinder.** Re-running every probe with collections suppressed
(`PERRY_GC_HEAP_LIMIT` beyond the workload) leaves the deltas essentially
unchanged (string-retention +35.0% suppressed vs +35.7% normal;
deep-stack +22.9% vs +22.8%), and `PERRY_GC_DIAG` shows *identical cycle
counts per probe across arms* — so it is neither mutator codegen cost nor
collection-frequency skew. Wait: suppression leaving the delta intact
would normally implicate the mutator — but the `perf` profiles resolve
it. The statepoint arm's top symbols are dominated by
`libunwind::CFI_Parser::parseCIE`, `getEncodedP`, `getULEB128`,
`findFDE` (8.7% + 6.0% + 4.1% + 3.0% on string-retention alone); the
shadow arm has none. The GC still runs its fixed cycle count under
suppression (the limit raises the trigger, it does not disable the
collector), and every one of those cycles walks the stack with the
platform unwinder because the fast chain is disqualified on Linux. **The
measured cost is DWARF CFI parsing per collection, not the statepoint
model.**

That is a configuration cost with two known remedies (an indexed
walker, or the Linux fast chain via upstream FP-relative spills), and it
means the Pi number must NOT be read as "statepoints are 15% slower on
small hardware." An attempt to confirm by relinking against libgcc's
unwinder instead of zig's bundled libunwind produced segfaulting
binaries (a hand-rolled link line missing the working recipe's flags) —
the implementation-vs-model split is therefore *measured to be unwinder
walking* but the specific unwinder's contribution remains unquantified.

Consequence for the campaign verdict: shadow retains three-axis
optimality including small hardware, and a future default-flip needs a
Pi-class gate — but the gap's cause is a named, fixable walker cost.

## Real-application scale (Claude Code 2.1.112, 2026-08-01)

The campaign's origin target — the real 13 MB minified `@anthropic-ai/claude-code`
bundle — compiles and runs natively under the shadow stack (204,103,064 B
binary, `__text` 149.4 MB, 115.1 MB RSS, `--version` correct, 82 min).

**The explicit statepoint bridge cannot compile it**, and the reason is a
scaling defect worth recording precisely:

- Statepoint lowering roughly doubles module IR: **1,083 MB, 16,748
  functions (~66 KB/fn)** for a 13 MB input.
- `clang -c` rejects the oversized unit outright: *"file … is too large for
  Clang to process."*
- **Adding codegen units does not fix it.** `decide_codegen_units` sizes by
  *callable count* (`ceil(fns / 6000)`), never by IR bytes; and
  `render_codegen_units` replicates **all shared string constants and
  globals into every unit**. At `PERRY_CODEGEN_UNITS=16` each unit still
  rendered ~370–436 MB, and unit 10/16 failed the same way — while total
  emitted IR ballooned past 6 GB (which also exhausted the disk mid-run
  and killed an earlier attempt).

Two independent fixes fall out, both mode-agnostic wins: size codegen units
by estimated IR bytes rather than callable count, and emit shared
strings/globals **once** with external declarations in sibling units
instead of replicating them. The second is what makes splitting actually
scale for string-heavy minified bundles.

Fairness note for anyone extending this: unit count changes cross-unit
inlining scope, so a statepoint arm forced to N units must be compared
against a shadow arm at the *same* N, not against the auto-chosen count.
The `-Os` downgrade (`module IR > 6 MB`) is NOT a confound — it applies to
both arms, since shadow IR for the same program cannot be smaller than the
statepoint arm's 1,083 MB.

## Pi 5 re-measurement after the prologue-decode walker + main rebase (2026-08-02)

The small-hardware regression is **closed and inverted**. Same host (Pi 5,
aarch64 Linux, load <1), same method (9 interleaved reps, per-probe
medians), both arms cross-built from one tree and one runtime archive:

| Probe | before: shadow / statepoint | after: shadow / statepoint |
|---|---:|---:|
| Nursery churn | 385.7 / 402.0 (+4.2%) | 392.4 / 391.9 (−0.1%) |
| Survivor promotion | 448.4 / 446.7 (−0.4%) | 286.1 / 277.1 (−3.1%) |
| Cross-gen writes | 428.4 / 461.0 (+7.6%) | 374.8 / 367.4 (−2.0%) |
| Dead after deep stack | 932.1 / 1145.0 (+22.8%) | 1039.2 / 1017.6 (−2.1%) |
| Closure capture | 334.6 / 361.6 (+8.0%) | 449.8 / 436.3 (−3.0%) |
| String retention | 275.7 / 374.1 (+35.7%) | 240.9 / 240.5 (−0.2%) |
| Array grow/evacuate | 362.8 / 483.1 (+33.1%) | 226.9 / 225.6 (−0.6%) |
| Map/set side tables | 978.9 / 1139.9 (+16.4%) | 1070.3 / 1040.8 (−2.8%) |
| **geometric mean** | **+14.72%** | **−1.74%** |

Correctness first, as always: 8/8 under forced evacuation + verification
and 8/8 under `PERRY_STACKMAP_WALKER=verify` (fast x29 walk and the
platform unwinder visit the identical slot set) — the prologue-decoded SP
is right on the architecture where the constant-based approach was proven
impossible.

**Attribution, stated honestly: the walker is NOT the cause of the
improvement.** A direct A/B on the two worst probes — same binary, fast
chain versus `PERRY_STACKMAP_WALKER=unwind` — is a dead heat (0.24 s vs
0.24 s; 1.01 s vs 1.01 s). The DWARF CFI parsing that `perf` measured at
~22% of samples is simply no longer hot. The other variable between the
two runs is the rebase onto main's 64 commits of GC work (root-store
dominance #7192, from-space protection and forced-collection tooling #7196, and #7148's precise
safepoint drains replacing conservative-scan fallbacks), which plausibly
reduced how often the native stack is walked at all. Shadow itself got
faster on the same probes (469.2 → 429.2 ms geo), which is consistent with
that explanation and inconsistent with "the walker fixed it".

So: the prologue decode is *correct and verified* and removes a real
fallback, but the measured win belongs to main's GC hardening. Both are
recorded rather than conflated.

★ One instrument failure worth recording: a first attempt to count GC
cycles reported **0 cycles for both arms**, which would have made the whole
comparison vacuous. It was a grep pattern that no longer matched main's
changed diagnostic format — raw output shows 81.9 MB freed across 79 arena
blocks. Never trust a count without looking at what produced it.

**Conclusion, stated as the design law this branch keeps re-deriving:**
*with an optimizing compiler between the source and the safepoint, root
metadata without relocation semantics is unsound — per-call plain maps
merely made the window small enough for probes to pass, and per-function
compact maps made it wide enough to fail in ten lines.* This upgrades
#7108's argument ("only statepoint describes the frame during the call")
from analysis to demonstration, and it means the metadata floor for a
sound non-statepoint scheme does not exist: the choice is statepoint-style
relocation (per-safepoint records, ~2× plain maps, the measured −59%
trajectory) or the shadow stack. The compact 10–13× is only reachable via
`RewriteStatepointsForGC`-style managed-pointer SSA — the toolchain
decision #7108 costed — or repsel shrinking the recorded set.

## Which statepoint design this tests

This is the explicit bridge, not LLVM's `RewriteStatepointsForGC` pipeline.
Perry emits the three intrinsics directly:

1. Load each live NaN-boxed `i64` root from its existing native alloca and
   temporarily convert the bits to `ptr addrspace(1)`.
2. Replace the original call with `llvm.experimental.gc.statepoint`.
3. Recover the call's scalar return through `llvm.experimental.gc.result`.
4. Recover every live root through `llvm.experimental.gc.relocate`, convert it
   back to `i64`, and store it to the original alloca.

The runtime collector executes inside the statepoint's callee. It unwinds to
the generated caller, finds LLVM's `Indirect` spill locations in
`__LLVM_STACKMAPS`, and rewrites those words during evacuation. The generated
caller then reloads the rewritten words through `gc.relocate`.

The small standalone version is
[`statepoint-bridge-probe.ll`](statepoint-bridge-probe.ll).

This choice intentionally avoids colliding with the representation experiment.
A full `RewriteStatepointsForGC` integration wants managed pointers to be
identifiable throughout SSA and expects a compiler pass to discover and
rewrite safepoints. Perry currently carries GC-capable values as NaN-boxed
`i64` words, so the bridge changes their representation only across one call.

## Why LLVM calls it experimental

The `llvm.experimental.*` prefix means LLVM does not promise a permanently
stable IR or binary interface across releases. It does not mean that the
mechanism is an abandoned toy or that production-oriented runtimes cannot use
it. For Perry it creates an engineering requirement: pin and test supported
LLVM versions, verify emitted IR, and treat upgrades as an ABI migration.

The relevant upstream contracts are
[Garbage collection safepoints](https://llvm.org/docs/Statepoints.html) and
[Stack maps and patch points](https://llvm.org/docs/StackMaps.html).

## Implementation

The prototype reuses the precise-root discovery and conservative per-call CFG
liveness built for the plain-stack-map experiment.

- Functions with roots receive `gc "statepoint-example"`.
- Ordinary direct calls with scalar arguments and scalar/void results are
  rewritten explicitly.
- LLVM intrinsics and compiler-only inline assembly are not safepoints.
- Unsupported call forms retain the plain `llvm.experimental.stackmap`
  fallback.
- A function containing Perry's setjmp-based `try` lowering uses the plain-map
  backend for the whole function.
- The runtime parser accepts plain-map `Direct` alloca addresses and statepoint
  `Indirect` spill locations. It deduplicates identical base/derived locations
  before visiting roots.
- The module retains each Mach-O `__LLVM_STACKMAPS` atom with
  `.no_dead_strip`.
- `PERRY_STATEPOINTS` participates in both build and object cache keys.

When `PERRY_STATEPOINTS=1` and `PERRY_STACK_MAPS=1` are both present,
statepoints take precedence in eligible functions.

## Correctness and coverage

Final release artifacts passed:

- all 8 GC-ratchet probes with stdout identical to Node;
- all 8 probes with `PERRY_GC_FORCE_EVACUATE=1` and
  `PERRY_GC_VERIFY_EVACUATION=1`;
- LLVM 22 verification of all eight generated modules;
- compilation of the minimal bridge with Apple clang 21.0.0 and LLVM 22.1.4;
- 314 `perry-codegen` library tests;
- 5 focused runtime stack-map/statepoint parser and call-site tests;
- the object-cache statepoint environment-key test.

Final generated-IR coverage:

| Probe | Statepoints | Relocations | Plain fallbacks |
|---|---:|---:|---:|
| Nursery churn | 152 | 227 | 0 |
| Survivor promotion | 165 | 296 | 0 |
| Cross-generation writes | 168 | 244 | 0 |
| Dead after deep stack | 119 | 135 | 0 |
| Closure capture | 146 | 191 | 0 |
| String retention | 92 | 95 | 0 |
| Array grow/evacuate | 100 | 100 | 0 |
| Map/set side tables | 138 | 274 | 0 |
| **Total** | **1,080** | **1,562** | **0** |

The zero here describes these probes, not the backend's complete call-form
coverage. Indirect calls, aggregate signatures, unusual call-site attributes,
and setjmp functions can still take the deliberate plain-map fallback.

Retained heap and heap capacity match the shadow-stack arm byte-for-byte.
Promotions, freed bytes, and cycle counts also match. Two probes have tiny
copy-accounting differences (+3 objects/+208 bytes and -152 bytes), with
identical final retention; the other six match all checked GC counters.

One apparent intermittent relocation failure during development was a stale
`target/*/libperry_runtime.a`. Perry executables link the
`perry-runtime-static` package, not the `perry-runtime` rlib directly.
Rebuilding only the latter left the old scanner in generated binaries. The
final results rebuild both the compiler and static runtime archive.

## Performance

Hardware was an Apple M1 Max on macOS 26.5. The interleaved run reported load
averages of 22.71/25.83/27.77, so these results are directional and should not
be promoted to release claims.

Each runtime cell is the median of 11 executions. Mode order was interleaved
and rotated after one warmup, and outputs were checked for equality before
timing.

| Probe | Shadow | Plain stack map | Statepoint | Statepoint vs plain |
|---|---:|---:|---:|---:|
| Nursery churn | 182.026 ms | 177.968 ms | 175.573 ms | -1.35% |
| Survivor promotion | 216.247 ms | 208.667 ms | 208.192 ms | -0.23% |
| Cross-generation writes | 210.743 ms | 203.775 ms | 204.504 ms | +0.36% |
| Dead after deep stack | 460.249 ms | 470.665 ms | 501.690 ms | +6.59% |
| Closure capture | 173.719 ms | 163.076 ms | 162.902 ms | -0.11% |
| String retention | 130.773 ms | 133.166 ms | 136.794 ms | +2.72% |
| Array grow/evacuate | 171.915 ms | 175.039 ms | 172.519 ms | -1.44% |
| Map/set side tables | 460.879 ms | 444.031 ms | 466.627 ms | +5.09% |

Geometric means versus shadow:

- plain stack maps: -1.66%;
- statepoints: -0.27%;
- statepoints versus plain maps: +1.42%.

The deep-stack result is the strongest negative signal. Both native-stack
backends pay for unwinding, while statepoints additionally materialize
relocation spill/reload state around a large number of calls.

Three uncached compilations per probe measured a +1.47% geometric mean for
plain maps and +2.12% for statepoints versus shadow. Sequential RSS
measurements put statepoints at roughly +1.03% median RSS and +0.70% peak RSS,
but allocator and host noise make those figures less reliable than retained
heap.

Statepoint metadata is materially larger:

| Probe | Plain payload | Statepoint payload |
|---|---:|---:|
| Nursery churn | 7,104 B | 13,656 B |
| Survivor promotion | 8,224 B | 16,112 B |
| Cross-generation writes | 7,768 B | 15,040 B |
| Dead after deep stack | 5,128 B | 14,824 B |
| Closure capture | 9,464 B | 18,440 B |
| String retention | 6,016 B | 10,912 B |
| Array grow/evacuate | 5,840 B | 11,480 B |
| Map/set side tables | 7,288 B | 13,840 B |

The total is 114,304 bytes versus 56,832 bytes, or 2.01x. Most executables are
about 16 KiB larger than shadow after Mach-O segment rounding; closure capture
crosses another segment boundary and is about 33 KiB larger.

## Is the GC simpler?

Relocation is simpler to reason about, but the GC system is not simpler yet.

The improvement is real: a statepoint makes the original call result and every
post-call root explicit SSA results. Plain maps required empty inline assembly
memory barriers to stop LLVM from caching root values across a call whose
stack slots the compiler could not know the collector mutates.

However, this bridge still needs:

- Perry's root discovery, logical slots, and conservative CFG liveness;
- addressable root allocas and per-statepoint load/store bridges;
- the LLVM stack-map v3 parser and native unwinder;
- Mach-O section discovery and linker-retention directives;
- call-form parsing and plain-map fallbacks;
- target- and toolchain-specific verification.

It removes generated shadow-frame push/pop and TLS slot mutation, but replaces
that local machinery with a wider compiler/linker/runtime contract. The
collector itself is nearly unchanged; only its root source changes.

## Is Perry better positioned?

Semantically, yes. Operationally, not enough yet to switch.

Statepoints provide the right vocabulary for a future moving collector:
relocation is explicit, base/derived relationships have a representation, and
a later managed-pointer pipeline can keep roots in SSA rather than forcing
Perry to invent compiler barriers.

This implementation is still a bridge with important liabilities:

- arbitrary NaN-boxed bits temporarily masquerade as managed pointers;
- all ordinary calls with live roots are treated as potentially allocating;
- indirect and unusual calls fall back to plain maps;
- `try`/setjmp functions fall back wholesale;
- scanning is macOS/Mach-O-only;
- active-frame matching still uses a 16-byte nearest-PC tolerance;
- the intrinsic and metadata contracts require LLVM-version discipline;
- the current measurements show no all-around speedup.

## Recommended next step

Do not replace the shadow stack from this branch. Preserve the prototype as
evidence and wait for the representation work before choosing the production
path.

After that work lands:

1. Define a safepoint-capability table so only calls that can enter the
   allocator become statepoints.
2. Represent genuine managed references directly instead of converting every
   possible NaN-box root through `inttoptr`.
3. Compare direct explicit emission with `RewriteStatepointsForGC` on that
   representation.
4. Remove plain-map fallbacks one call form at a time and fail closed on
   unsupported targets.
5. Re-run the 11-way interleaved suite on an idle pinned host, with separate
   profiles for mutator root maintenance, relocation reloads, unwinding, and
   collector root scanning.

## invoke-EH lands on main; try functions covered (2026-08-03)

main replaced setjmp/longjmp exception lowering with `invoke`/`landingpad`
(#7302, PR #7305) and deleted `volatile_setjmp.rs` and `setjmp_abi.rs`. That
retires this branch's correctness blocker: a `longjmp` could jump past a
`gc.relocate`, which is why try-carrying functions were excluded from
statepoints and routed to the plain-stack-map lowering — itself unsound, since
LLVM may record a root slot's address in a caller-saved register that cannot
be recovered at collection time.

The exclusion was not merely obsolete but **unrepresentable**: main deleted the
`has_try` field, so the compiler forced its removal.

### The probe that had no equivalent

Nothing in the suite contained a `try` — 0 of 8 probes — so the newly covered
case was exercised by nothing at all, and a green run said nothing about it.
`09_try_catch_roots.ts` closes that: objects allocated inside a `try` surviving
a collection inside the same `try`; locals live across a throw and read in the
`catch`; a throw crossing several frames so the rewritten roots sit in a
caller's frame; `finally` on both edges; and a rethrow caught one frame up.
Every survivor folds into the checksum, so a lost or stale root is a wrong
number rather than a crash.

Its map is 1,116 bytes — the largest of any probe — which is the liveness
evidence that try-carrying functions now genuinely carry statepoint records.

### Result, and a real limitation it exposed

**Explicit statepoint bridge: 9/9**, byte-matching the pinned Node oracle
normally and under `PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1
PERRY_STACKMAP_WALKER=verify`.

**RS4GC: 8/9 — it cannot compile a try-carrying function.** The LLVM verifier
rejects the module:

```
%lpad = landingpad { ptr, i32 }
token  %r2.0.relocated = call coldcc ptr addrspace(1)
         @llvm.experimental.gc.relocate.p1({ ptr, i32 } %lpad, i32 1, i32 0)
```

`gc.relocate`'s first operand must be a `token`. RS4GC emitted the relocates on
the unwind edge against the landing pad's `{ ptr, i32 }` result, because
`statepoint-example` expects a statepoint-invoke's unwind destination to carry
`landingpad token` rather than the Itanium form `try_stmt.rs` emits.

This matters for the size ranking: **RS4GC is the leanest arm measured
(−131,624 B versus shadow) but cannot handle `try` yet, so the arm that is
actually complete today is the explicit bridge at −49,072 B.** Both still beat
the shadow stack on size; the RS4GC/EH interaction is the remaining work, and
it is an LLVM-convention problem rather than anything the compact map touches.

### Correction: the size win does not survive the merge with main (2026-08-03)

Re-measured on `test-drizzle-pg` after merging main, one compiler, clean
object cache per arm:

| arm | total | `__text` | gcmap | unwind | eh_frame | vs shadow |
|---|---:|---:|---:|---:|---:|---:|
| shadow | 26,950,504 | 19,801,644 | 0 | 195,104 | 1,479,652 | — |
| bridge | 26,951,000 | 19,650,408 | 189,454 | 187,056 | 1,375,028 | **+496** |
| RS4GC | 27,000,568 | 19,562,088 | 220,936 | 187,064 | 1,374,892 | **+50,064** |

Pre-merge the same measurement gave −49,072 (bridge) and −131,624 (RS4GC).
Main's own changes shrank every arm by ~1.7–1.8 MB, but shrank **shadow about
50 KB more than the statepoint arms**, which is the entire swing.

What did not change is the reason to keep the compact map: statepoints still
generate less code (`__text` −151 KB for the bridge, −240 KB for RS4GC, plus
~105 KB less `__eh_frame`). Those savings are simply now cancelled by the
189–221 KB of remaining metadata. Without compaction that metadata is 4.2 MB
and the arm loses by ~4 MB, so the 18–19× is doing real work — it converted a
3.5 MB loss into a tie, not into a win.

**Honest standing on the three axes**, post-merge:

* **performance** — statepoints ahead (−0.93% RS4GC, measured earlier);
* **RSS** — tied;
* **file size** — the explicit bridge is *tied* with shadow (+496 B, 0.002%);
  RS4GC is 50 KB behind despite the smallest `__text`, because its live-set is
  larger and so is its map.

Closing the last axis therefore needs the root set to shrink, not the encoding:
221 KB of map for 154k roots is already near this format's floor. That is the
repsel-promotion lever the earlier projection named, and it is still the
outstanding work.

## Safepoint density and the caller-frame constraint (2026-08-24)

A polling design cannot soundly mark every ordinary call as
`gc-leaf-function` under LLVM statepoints. If `A` calls `B`, and `B` reaches an
allocation or loop poll that starts moving collection, `A` is suspended at its
call to `B`. The collector must find and rewrite `A`'s live managed values at
that return PC. Omitting the statepoint on `A -> B` would remove exactly that
caller-frame relocation map; putting a poll only inside `B` does not recreate
it. VM poll points reduce where collection may begin, but every active caller
edge beneath such a poll still needs an oop/relocation map.

Perry therefore applies the maximal local reduction that preserves this
constraint: compute a whole-module, greatest-fixed-point GC-effect closure and
mark a direct generated call leaf only when its callee cannot transitively
reach collection. The proof admits mutually recursive pure components. It
fails closed on any allocation or poll helper, indirect call, unknown external,
or cross-module call, and propagates that result back through callers. Runtime
helpers remain governed by the audited `GcCallEffect` table.

The closure is computed before codegen-unit partitioning and carried into every
unit, so a safe direct edge remains leaf even when caller and callee are emitted
into different objects. Textual and native LLVM construction consume the same
set; the native dialect also preserves the marker on `invoke` edges inside
`try`. Calls outside the proven set remain ordinary RS4GC safepoints. Reducing
those further requires a different frame representation (for example, spilling
caller roots to a shadow frame), not merely moving the collection trigger.
