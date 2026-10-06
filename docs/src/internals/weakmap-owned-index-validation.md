# WeakMap owned storage: implementation validation

**Status: implementation complete; performance acceptance blocked by TypeScript
cycles. This is an unaccepted review candidate, not a shippable performance change.**

The implementation is rebased onto pristine main
`68e41faa0f9145d8b8ab67ecb0cf414eaf13420b`. This includes c85c2db93 (#12105), the LLVM statepoint-root correction.
The rebase preserves main's persistent-symbol GC type and census initializer;
owned weak storage uses the next free GC type (33). The root-holder inventory
keeps both current-main audits and conditional-tracing audits.

## Root cause and implementation

A hit previously revalidated generic array lengths and elements, searched an
external owner-address index, and then extracted the entry again. The GC also
followed a WeakMap value strongly before establishing independent key liveness,
allowing a value-to-key cycle to retain its own key (#12087).

WeakMap/WeakSet now own one GC storage cell through their object metadata.
Their intrinsic brand lives in the Shape and survives Shape transitions.
Authoritative key/value words remain in the cell; buckets and reverse offsets
contain scalar indices. Every get, has, set, and delete classifies the key
once, carries one validated view, and probes once. Growth roots arguments and
reloads them after allocation. The has/get pattern uses two inexpensive
independent probes without a last-key cache or external ownership registry.

Conditional ephemeron closure follows values only after independently live
keys. Moving collectors rewrite both words and rebuild identity buckets before
resuming the mutator. Weak writes maintain generational coverage without
unconditionally shading copied entries. Receiver-brand, exotic-key, symbol,
WeakRef, and FinalizationRegistry checks remain in place.

## Measurement method

Fresh main and fix compiler/runtime/stdlib static wrappers use separate targets
on qb6. Compiles use CPUs 0–55 and at most eight workers. All source files in
both remote arms were checked against the local Git/source hashes before
building; TypeScript is explicitly compiled from its actual npm source rather
than routed to a native shim. The real-program sources were copied unchanged
from the captures2 lane; nothing was written there.

Each acceptance row uses five main, five fix, and five identical-main
floor runs, with randomized interleaving; TypeScript has ten of each. Measurement batches hold qb6's
MEASURE.lock, pin CPU 56 within the reserved 56–63 set, and disable ASLR.
Batches are limited to 45 minutes with TypeScript, otherwise 30 minutes.
`instructions:u` and `cycles:u` are perf user-mode counters. CPU is perf
task-clock in milliseconds and is informational; RSS is GNU time peak KiB.
Acceptance follows the owner-approved PERF_POLICY_2026-10-06.md, which
replaces fixed per-axis ceilings: explain real-program deltas beyond the
same-binary floor, retain correct Shape/ownership architecture, and name a
fix-forward plan for any small architectural regression. Signed identical-main
floors are reported separately for instructions, cycles, and RSS.

GC counts accompany RSS but come from three separate runs of the same
uninstrumented executable. A ptrace reader stops its own child only at exec
and exit, then reads the runtime's existing GC_STATS TLS. It adds no mutator
counter or collection hook. The constant-output hello binary contains no
runtime and performs zero collections. Diagnostic GC logs and DWARF profiles
are separate from acceptance counter/RSS runs.

The Effect input uses exactly `import { Schema } from "effect"`, pinned to
4.0.0-beta.83, constructs 2000 schemas and decodes 20000 values. Both arms must
print `ok=20000`. Zod runs 5000 parses. Package versions are Node 26.5.1, Bun 1.3.14, Zod 3.25.76,
TypeScript 5.9.3, qs 6.16.0, Commander 15.0.0, and Fastify 5.12.5.

The coordinator-directed buffer_heavy/worker_heavy skips are recorded: both
fail on main and belong to other sessions. They were not patched or substituted.

## A/B medians

| Program | Instructions M main → fix (Δ%) | Cycles M main → fix (Δ%) | CPU ms main → fix | RSS KiB main → fix (Δ KiB) | Minor/full GC main → fix |
|---|---:|---:|---:|---:|---:|
| weakmap-get | 825.229 → 240.689 (-70.834%) | 287.808 → 84.158 (-70.759%) | 79.770 → 25.680 | 24304 → 23868 (-436) | 0/0 → 0/0 |
| weakmap-hit-pair | 1613.228 → 432.439 (-73.194%) | 564.394 → 148.638 (-73.664%) | 154.790 → 42.670 | 24308 → 23868 (-440) | 0/0 → 0/0 |
| weakmap-set | 1193.232 → 278.189 (-76.686%) | 239.388 → 57.333 (-76.050%) | 66.010 → 17.750 | 24304 → 23868 (-436) | 0/0 → 0/0 |
| weakset-has | 822.230 → 231.189 (-71.883%) | 160.580 → 45.933 (-71.396%) | 45.940 → 15.050 | 23860 → 24312 (+452) | 0/0 → 0/0 |
| weakset-add | 1330.230 → 294.189 (-77.884%) | 265.585 → 59.065 (-77.760%) | 73.290 → 18.190 | 23860 → 24312 (+452) | 0/0 → 0/0 |
| Effect | 24436.351 → 22754.859 (-6.881%) | 8839.858 → 8271.501 (-6.429%) | 2375.020 → 2224.220 | 189392 → 181676 (-7716) | 18/2 → 24/1 |
| hello | 0.250 → 0.250 (+0.000%) | 0.390 → 0.391 (+0.256%) | 0.870 → 0.880 | 1592 → 1592 (+0) | 0/0 → 0/0 |
| 07_object_create | 32.734 → 32.734 (-0.000%) | 17.377 → 17.111 (-1.532%) | 7.050 → 6.960 | 26968 → 26556 (-412) | 0/0 → 0/0 |
| 09_method_calls | 61.129 → 61.129 (+0.001%) | 121.627 → 121.611 (-0.013%) | 34.470 → 34.450 | 23900 → 23776 (-124) | 0/0 → 0/0 |
| 14_closure | 250.981 → 250.982 (+0.000%) | 151.436 → 151.478 (+0.028%) | 41.910 → 41.940 | 14588 → 14724 (+136) | 0/0 → 0/0 |
| bench_object_property | 174.054 → 174.087 (+0.019%) | 39.675 → 39.389 (-0.719%) | 13.680 → 13.520 | 34136 → 33692 (-444) | 1/0 → 1/0 |
| zod | 1328.640 → 1331.206 (+0.193%) | 434.689 → 435.059 (+0.085%) | 118.850 → 118.900 | 50724 → 50748 (+24) | 6/0 → 6/0 |
| tsc | 256258.986 → 256757.935 (+0.195%) | 94816.300 → 98686.655 (+4.082%) | 25311.570 → 26327.035 | 271152 → 271512 (+360) | 26/14 → 26/14 |
| qs_parse_nested | 22306.853 → 21645.601 (-2.964%) | 7955.556 → 7618.069 (-4.242%) | 2115.530 → 2026.040 | 52608 → 52436 (-172) | 129/0 → 130/0 |
| qs_stringify_nested | 57116.054 → 51186.191 (-10.382%) | 17106.529 → 15278.690 (-10.685%) | 4537.630 → 4055.180 | 54020 → 52216 (-1804) | 439/0 → 408/0 |
| commander_parse_argv | 7151.612 → 7156.157 (+0.064%) | 2694.822 → 2706.840 (+0.446%) | 718.050 → 720.570 | 46124 → 46264 (+140) | 31/0 → 31/0 |
| fastify_inject | 109761.894 → 109700.164 (-0.056%) | 43309.484 → 43223.539 (-0.198%) | 11531.300 → 11506.350 | 254544 → 254868 (+324) | 11/1 → 11/1 |

Signed identical-main floor/main median deltas:

| Program | Instructions floor % | Cycles floor % | RSS floor % (KiB) |
|---|---:|---:|---:|
| weakmap-get | -0.00009 | +0.429 | +0.000 (+0) |
| weakmap-hit-pair | +0.00008 | -1.643 | +0.000 (+0) |
| weakmap-set | -0.00005 | -0.149 | +0.016 (+4) |
| weakset-has | -0.00002 | +0.671 | +0.000 (+0) |
| weakset-add | +0.00001 | -0.227 | +0.000 (+0) |
| Effect | -0.00052 | +1.863 | -0.106 (-200) |
| hello | +0.00040 | -0.369 | +0.000 (+0) |
| 07_object_create | +0.00049 | -1.402 | +0.000 (+0) |
| 09_method_calls | -0.00003 | -0.046 | +0.000 (+0) |
| 14_closure | -0.00000 | -0.025 | -0.027 (-4) |
| bench_object_property | -0.00012 | +0.466 | +0.000 (+0) |
| zod | -0.00080 | -0.104 | +0.000 (+0) |
| tsc | +0.00204 | -0.186 | +0.641 (+1738) |
| qs_parse_nested | +0.00078 | +1.638 | +0.000 (+0) |
| qs_stringify_nested | +0.00134 | -0.464 | +0.000 (+0) |
| commander_parse_argv | +0.01583 | -0.056 | -0.009 (-4) |
| fastify_inject | +0.01990 | -0.144 | -0.024 (-60) |

## Why Effect performs more minor collections

The historical 10 → 25 total was 8 minor/2 full → 24 minor/1 full.
The main arm then retained value-to-key cycles because WeakMap values were
strongly traced before key liveness was known. Conditional tracing removes
that false reachability. The existing nursery retuner shrinks after two
cycles with less than 1% Eden survival; the historical fix consequently
shrunk from scale 16/4 to 1/4, while that main run ended before satisfying
both low-survival cycles. More roughly 4 MiB minors were expected from this
unchanged policy, rather than additional JS work or a new pacing heuristic.

The baseline-shifting #12105 merge requires fresh counts. On the current
baseline they are 18 minor/2 full → 24 minor/1 full (20 → 25 total), and both
arms eventually shrink their nursery. The fix's avoided second full is partly
replaced by small minors. At the first full, main retains 15,914,632 bytes
(15,333,120 mature), versus 6,011,240 (5,999,016 mature) with conditional
tracing. Allocation debt at that point is nearly equal: 134,987,032 versus
135,084,352 bytes (+0.072%). The fix shrinks 16/4 → 1/4 at 4200 Eden-live
bytes; its resulting nursery band is 3,783,262 bytes.

The storage representation also moves the identity buckets into GC ownership:
large GC allocation accounting at the first full is 1,540,352 → 5,435,840
bytes. The old external index bytes were outside that accounting. This is a
representation change, not an external cache or independently retained key.
No nursery sizing, tenuring, full-GC threshold, or reclaim policy was changed.
The whole-program instruction/cycle/RSS measurements decide the net cost;
collection count alone is not a performance regression.

## Mechanisms and remaining acceptance issue

Effect and the WeakMap/WeakSet repros remove repeated generic entries-array
validation, the address-owner index, and entry extraction after lookup.
The instruction profiles identify `index::find`, `js_array_length`, and
`js_array_get_f64` below the old WeakMap hits; those descendants disappear
from the new hits. On sampled Effect instructions, weak-operation ancestry
falls from 4.31% to 0.87%. These are sampled ancestry shares, not additive
whole-program counter budgets: recording was throttled and does not cover
the whole executed instruction stream. Unprofiled perf-stat rows above are
the acceptance measurements.

The qs drivers execute `side-channel-weakmap` through borrowed prototype
methods. Both time profiles show its calls below get/has/set; the old
`index::find`/free-slot index paths disappear in fix. Stringify also allocates
fewer GC entry objects: its minor count falls 439 to 408, with no full
collection in either arm. Parse has 129 to 130 minors, but its instructions,
cycles, and RSS all decrease. The extra minor is a collection-boundary change,
not a new pacing policy. The THP-disabled stringify check is recorded below
because its primary RSS delta is close to 2 MiB.

Zod's +0.193% instructions and Commander's +0.064% are small architectural
costs in ordinary-object workloads: intrinsic weak-brand admission now reads
the Shape instead of accepting inherited class identity, and traced objects
encounter the additional owned-weak-storage type decision. Zod executes
96,351 `collection_brand` calls in a separate GDB count; main executes
228,608 parent-class queries across all callers. Its instruction profile
moves the parent-class-query self share from 1.13% to 0.57% and introduces
0.67% Shape-brand reads. Both arms have six minor collections. This identifies
the changed admission/trace work; it is not a claim that profiling apportions
every one of the 2.57M extra instructions exactly. Commander likewise keeps
31 minor/0 full collections. Fix-forward: coordinate with native-family to
carry one admitted ObjectHeader/Shape proof through weak-brand dispatch,
and with the GC scan owner to carry the already-read GC kind into conditional
tracing. Keep all wrong-brand and exotic-receiver checks; add no cache or
name-based shortcut.

Fastify's instruction/cycle decreases are small, but exceed their recorded
floors. Its generic method dispatch now uses the Shape weak-brand negative
check; profiles show parent-class-query self share 0.51% to 0.34% and the new
Shape read at 0.19%. The full/minor regime remains 1/11. Its +324 KiB peak RSS
is accompanied by +336 KiB file-backed residency in the separate exit
snapshot, consistent with changed code residency. Zod (+24 KiB) and Commander
(+140 KiB) also have unchanged GC regimes and larger file-backed residency;
no ordinary-object or Shape header was enlarged.

The short controls have file-backed residency changes, not a per-object
storage tax. For the closure control, the sampled RSS peak has 8784 to 8788
KiB anonymous memory, while file residency changes by hundreds of KiB.
WeakSet-has has exactly 17004 KiB anonymous memory at both sampled peaks;
its file-backed peak varies with code-page residency. The linked text grows
by 15,184 bytes for WeakSet-has and 15,160 bytes for the closure control;
ordinary ObjectHeader and ShapeRecord remain 64 bytes. Primary micro RSS
increases are +452 KiB for both WeakSet rows and +136 KiB for closure.
These are logged under the replacement policy, not rejected using the old
percentage/256-KiB gate. Follow-up for the layout owner: measure transient
bootstrap image residency and placement on these tiny executables.

TypeScript is the unresolved acceptance issue. Its ten-run primary row is
+0.195% instructions, +4.082% cycles, +360 KiB RSS; the identical-main floors
are +0.00204% instructions, -0.186% cycles, and +1738 KiB RSS. Both arms have
26 minor and 14 full collections. All 31,025 generated TypeScript functions
have equal sizes; 30,782 change addresses by -1648 bytes when the runtime is
relinked. A separate three-round hardware-counter diagnostic reproduces
+4.46% cycles and shows cache misses 388.9M to 448.0M (+15.2%), versus an
identical-main floor of -5.0%; branch misses rise 107.7M to 119.1M. This
points to changed code/data placement and cache locality, rather than extra
JS work or additional collections. It does not yet isolate which placement
change is responsible. The THP-disabled diagnostic below tests the page-step
alternative. A remaining multi-percent real-program CPU regression requires
owner review under policy section 4; it is not silently accepted because
instructions are primary or because the other workloads improve.

## THP-disabled diagnostics

The original TypeScript RSS floor is +1738 KiB and stringify's RSS delta is
-1804 KiB, so both were checked with per-process `PR_SET_THP_DISABLE`, without
changing the host's global THP policy. The child snapshots confirm
**AnonHugePages=0 KiB** in both arms. These are additional diagnostics, not
substitutes for the ten-run TypeScript primary row.

| Program / runs per label | Instructions M main → fix (Δ%; floor) | Cycles M main → fix (Δ%; floor) | RSS KiB main → fix (Δ KiB; floor) | Minor/full main → fix |
|---|---:|---:|---:|---:|
| TypeScript / 3 | 256204.837 → 256750.533 (+0.213%; +0.00233%) | 96511.768 → 98725.429 (+2.294%; -1.021%) | 233788 → 234044 (+256; 0) | 26/14 → 26/14 |
| qs stringify / 5 | 57116.016 → 51191.512 (-10.373%; +0.01536%) | 17080.877 → 15294.530 (-10.458%; -0.124%) | 27872 → 27096 (-776; 0) | 439/0 → 408/0 |

Stringify still reduces RSS with THP disabled. The larger primary -1804 KiB
reduction is sensitive to huge-page residency; exit AnonHugePages is not a
measurement of residency at the RSS peak. TypeScript still has a
multi-percent cycle regression beyond its same-binary floor. Cache misses
also remain elevated with THP disabled (420.9M → 468.9M, +11.4%, versus a
-5.17% identical-main floor). The page-step hypothesis does not remove the
CPU blocker.

## Node and Bun references

Separate three-run exact-Effect references on the measurement core:
Node 26.5.1: 3677.870M user instructions, 595.870 ms CPU, 211868 KiB RSS;
Bun 1.3.14: 3303.585M user instructions, 516.430 ms CPU, 158780 KiB RSS.
All print `ok=20000`. Perry main/fix are 24436.351M / 22754.859M above.

The same WeakMap-get source, with startup subtracted by instruction slopes,
costs **824.000 → 239.500 user instructions/iteration** on Perry (five runs
at 1M and 2M); current Node/Bun three-run slopes at 100M and 200M are
128.942 / 61.009. These are complete loop/selection/checksum pattern costs,
not naked runtime-helper counts, and Node's JIT tiers add variability.
The Perry/Node gap drops from 6.39x to 1.86x on this baseline. The historical
842/114 table remains historical rather than a fresh-baseline comparison.

## Acceptance and next work

This change removes the external owner-address WeakMap index and strong
value tracing, replacing them with collection-owned storage and conditional
ephemeron tracing.

The old control-RSS percentage gate no longer applies. All required programs
were built and measured, including the previously OOM-blocked actual
TypeScript package. The remaining blocker is its +4.082% primary cycles
(+2.294% with THP disabled), with instruction increases of +0.195% / +0.213%.
Section 4 of the replacement policy sends a larger real-program regression
to the owner; this result is neither an automatic pass nor the old blanket
+0.5%/256-KiB rejection. Increased cache misses are measured, but the specific
placement conflict is not isolated. Next work for the layout/GC/native-family
owners is to locate the added miss sites and separate linking placement from
necessary admission/trace work. Retain the owned storage and conditional
semantics; do not regain speed with an owner-address registry, a last-key
cache, a name-based shortcut, or unconditional value tracing.

The two-commit series and bundle preserve this complete implementation for
review. They are explicitly unaccepted until the CPU issue is resolved or
the owner decides the explained trade. Build targets are removed after all
evidence is copied. No push or PR is performed.

## Correctness and repository checks

Fresh release runtime suite: fix **5208 passed, 0 failed, 5 ignored**;
integration test **1 passed**. Main: **5195 passed, 1 failed, 5 ignored**.
Main's pre-existing failure is
`gc::tests::native_payload_streams::z8_churn_releases_every_codec_at_completion_and_drops_every_payload`:
all 4000 codecs/cells were released/finalized, but the anonymous-RSS warmup
threshold assertion saw 4,333,568 bytes growth. Fix's full suite passes;
there is no new runtime failure to excuse.

Both #12087 witnesses compile on pristine main, pass their movement/control
preconditions, and fail at the intended assertions: the conditional value
keeps its own key alive (raw key remains non-undefined; the entry remains
live). Both pass with fix. The filtered fix run also passes indirect/cross-map
and Proxy-chain witnesses (four tests total). Main's source file is restored
by a trap after the failure proof.

All six previously Node-matching weak-area files still match. The new
72-line behavior matrix matches Node on fix both normally and under
`PERRY_GC_STRESS_SEED=7 PERRY_GC_STRESS_RATE=1 PERRY_GC_TRACE=1`:
57 actual copying minors move **38,421 objects**. The existing
`test_issue_2656_weakref_finalization_gc.ts` differs from Node on both arms;
it is not a newly matching file or a newly introduced mismatch.

`cargo fmt --all -- --check` and Node-version consistency pass.
The root-holder inventory has the same four pre-existing unregistered
thread-local declarations on main/fix: IN_SAMPLER, BLOCK_PERSIST_FORCE_MARKS,
TAPE_REGISTRY, TAPE_REGISTRY_NONEMPTY. Address-class audit failures match main
apart from source line shifts; file-size audit has the same six offending
paths/line counts on both arms. Their raw main/fix logs and comparison proofs
are retained. Do not present those existing failures as green checks.
No codegen file or extern-C signature/effect changed, so the conditional
codegen-suite and gc_call_effects requirements do not apply.

Build inputs, compiler/static-wrapper/binary hashes, raw perf/time runs,
three-run GC snapshots, Node output parity, witness failures, and tests are
retained under scratchpad `weakmap/evidence-oct7`. The main source archive
comes from 68e41faa; the two targets are separate. No push or PR was made.
