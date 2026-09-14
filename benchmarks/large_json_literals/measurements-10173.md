# Mid-size record literal cliff (#10173)

The 3,200-record fixture has 22,401 AST value nodes, below the existing
24,576-node record cutoff. It still lowers to `__AnonShape_*` HIR, preserving
typed property reads. This change replaces its construction code with a static
descriptor and a shared runtime materializer. It does not lower the JSON.parse
cutoff or change property-read codegen.

## Attribution

Linux x86_64, LLVM 22.1.8, release compiler at `4a3c8ec9e2` (v0.5.1568).
Both arms were built in this lane with the identical package set:
`cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static`.
Each compiler and its freshly built archives were pinned together, with
`PERRY_RUNTIME_DIR` set explicitly. The baseline includes only the diagnostic
addition that reports optimized instruction counts below the existing budget.
All builds, tests and execution ran on the Linux host through `remote.sh`.

The fresh-cache 3,200-record baseline takes **95.49 s / 1,440 MiB** here. Its
critical codegen unit has these timings:

| Phase | Time | Evidence |
| --- | ---: | --- |
| Lowering function bodies to LLVM IR | 0.4 s | The synthetic four-field constructor itself takes <0.1 s |
| RS4GC preprocessing | 0.7 s | 774,396 → 723,186 instructions; the existing relocation estimate selects shadow roots up front |
| LLVM IR optimization | **88.27 s** | 650,941 instructions remain in the initializer |
| Machine emission, default 600,000-instruction cap | 2.8 s | The single-function unit uses the O0 machine pipeline |

A separate `perf record -F 99 --call-graph dwarf,8192` run corroborates the
phase attribution: 82.33 s optimizing, 0.7 s in RS4GC, 2.7 s emitting. The
compiler is stripped, so the pass timings, rather than unsymbolized sample
addresses, are the useful attribution. That run takes 92.43 s including perf
overhead. This is a new baseline, not a restatement of the issue's older 342 s
measurement.

The handoff's fast-emission lead was tested explicitly. Setting
`PERRY_LL_FAST_EMIT_MAX_INSTRS=0` takes **126.21 s / 1,906 MiB**: optimization
still costs 88.4 s and emission rises to 33.8 s. `.text` falls from 6,297,159
to 3,609,143 bytes, but compilation becomes slower. Raising that limit would
not fix this cliff. The default fallback already bounds its emission cost.

Other shapes hit different parts of the same expansion. html-entities named
references expands a wide constructor from 223,112 to **5,189,309 instructions**
in RS4GC and retries with shadow roots. MIME `other.ts` spends 339.13 s in IR
optimization on its initial timed-out run, then more than four minutes in
machine emission. HIR lowering takes only hundredths of a second there.

## Implementation and tradeoffs

Constant record trees with at least 256 HIR value nodes are serialized into
read-only data. The gate requires at least one record and proves each
constructor consists solely of ordered parameter-to-field stores. It declines
effects, user constructors, captures, computed properties, spreads and strings
requiring the separate WTF-16 path. Primitive-only arrays retain their existing
lowering. The speculative descriptor walk is depth-bounded.

The schema uses the existing class ID, rooted keys global, typed ShapeId global
and immutable raw/pointer masks. The materializer creates fresh mutable arrays,
strings and objects, fills fields in order, and validates the same typed layout
used by ordinary `new`. It contains no user callbacks and suppresses GC while
building partial trees, like the existing constant-array descriptor. Its table
holds addresses of registered module root slots, never cached heap pointers.

Wide synthetic constructors (32+ fields) still need a callable body even when
constant sites bypass them. They now marshal boxed arguments into a stack
buffer and call one shared strict-assignment loop; `noinline` prevents LLVM
from duplicating that marshalling body. The helper roots all operands before
the first assignment, rereads them after setters, and preserves strict
descriptor/prototype behavior. Arbitrary value expressions still evaluate on
the ordinary path. This addresses both huge initialization sites and huge
otherwise-unused constructor definitions. Chunking would retain many repeated
allocation/rooting/store sequences; static data removes those sequences while
keeping the existing object representation and read performance.

## Fresh-cache standalone measurements

`--no-link --no-auto-optimize`, default optimization and GC settings. `.text`
is the sum of `.text*` sections from `size -A` on the resulting module object;
it excludes shared runtime code. RSS is GNU time's maximum RSS, converted from
KiB to MiB. Each module resolves to exactly one input module; no full OpenCode
compile was run. Times are individual observations on a shared build host.

| Input | Seconds before → after | Peak MiB before → after | `.text` bytes before → after |
| --- | ---: | ---: | ---: |
| records-400 | 40.369 → 0.300 | 1,690 → 196 | 14,342,070 → 5,949 |
| records-3200 | 95.490 → 0.633 | 1,440 → 238 | 6,297,159 → 6,017 |
| MIME other.ts | 674.625 → 0.874 | 2,321 → 231 | 13,670,794 → 2,517 |
| MIME standard.ts | 63.740 → 0.834 | 836 → 222 | 3,419,680 → 1,637 |
| html-entities named-references.js | 263.806 → 1.762 | 4,062 → 302 | 5,069,754 → 70,266 |

The modules are MIME 4.1.0 `types/{other,standard}.ts` and html-entities 2.3.3
`lib/named-references.js`, from the read-only OpenCode dependency checkout.
The initial MIME `other.ts` run timed out at 600 s; the table uses its completed
rerun with a longer timeout.

| Input | Largest function instructions after IR optimization, before → after | Sum over all emitted units, before → after |
| --- | ---: | ---: |
| records-400 | 622,076 → 1,087 | 624,083 → 1,470 |
| records-3200 | 650,941 → 960 | 665,196 → 1,435 |
| MIME other.ts | 522,756 → 1,370 | 1,037,401 → 1,500 |
| MIME standard.ts | 144,416 → 666 | 277,972 → 796 |
| html-entities named-references.js | 171,797 → 5,023 | 669,610 → 20,950 |

For named references, the counts describe the successfully emitted units after
the baseline retry. Its largest remaining candidate function is a closure;
the other MIME maxima are the synthetic constructors. For records-3200 the
candidate maximum is the read loop; the descriptor initializer is smaller.

## Runtime: five alternating pairs per fixture

One warmup per arm, then five `before, after` pairs, pinned to the same CPU
(15). The initial scaling series overlapped another lane's release build and
was retained separately; the complete series below was repeated after that
build exited. Both arms use their own matching pinned archives.

| Fixture / loop | Before median | After median | Checksum, identical in both arms |
| --- | ---: | ---: | ---: |
| hot.ts numeric | 287 ms | 70 ms | 79,042,210,000 |
| hot.ts records | 275 ms | 169 ms | 2,011,000,000 |
| Isolated numeric | 70 ms | 70 ms | 79,042,210,000 |
| Isolated 400 records | 273 ms | 150 ms | 2,011,000,000 |
| Isolated 3,200 records | 1,246 ms | 1,176 ms | 128,088,000,000 |

The 3,200-record loop visits eight times as many records: 1,176 / 8 = **147 ms**,
within 2% of the 400-record 150 ms median. The combined-file speedup includes
recovering optimized machine emission for the read loops; it is not an
intrinsic numeric-array speedup. The isolated numeric arm is unchanged.

## Validation and reproduction

Passed: `cargo test -p perry-hir`; all ten `json_literal` cutoff tests;
`cargo test -p perry --test issue_10151_large_json_define`; three descriptor
codegen tests; descriptor and constructor runtime tests with
`RUST_TEST_THREADS=1`; build-cache environment registration; formatting,
file-size, address-classification, runtime-root inventory and Node-pin checks.
The 3,200-record integration regression has a 60-second compile limit, requires
static-shape HIR without `JsonParse`, and checks order, freshness, mutation and
collection between materializations. A codegen unit test also caps each emitted
function body, so a faster CI machine cannot hide the code-size cliff.

Forced-evacuation unit tests require an actual copying minor and changed object
addresses, then verify traced children and assignment after a collecting
setter. A compiled record/effectful-wide-record GC probe passes with both native
and shadow roots. Static checks of its shadow LLVM IR cover 89 root stores and
80 GC-capable allocas: zero dominance violations, fatal stale-register uses or
unrooted allocas. No root-checker exemptions were added.

Generate probes with `generate.py`, then use:

```sh
python3 benchmarks/large_json_literals/measure.py /path/to/pinned/before \
  /path/to/probes/records-3200.ts /path/to/results/before-records-3200
# Repeat with the candidate and the three standalone module paths.
# Add --profile for perf; add --fast-emit 0 for the emission control.
# Link hot/numbers/records/records-hot-3200 with --link into before-*/after-*.
python3 benchmarks/large_json_literals/alternate.py /path/to/results
```

Raw phase logs, `perf.data`, section/symbol sizes, commands, archive hashes and
all runtime samples are retained in the lane's `literal-results` directory.
The checked-in JSON records the measurements and executable/archive hashes.
Unverified: full OpenCode binary size/startup, other dependency modules, and
non-Linux targets; the requested standalone measurements do not establish them.
