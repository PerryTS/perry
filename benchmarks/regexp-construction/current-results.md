# RegExp cache: verification against Perry 0.5.1557

Runtime source `5b42a596f34b75800c9ec31abdb2526925a4b3f5` integrates
main `64f5249ac0a29eb7ffdde58947355b8a6bd38f27` (Perex 0.1.4).
The preserved candidate build snapshot is
`c03b3348e0905d2ef7d9ffe628508f3b0f216689`.
All builds, tests, probes and profiles ran on Linux through
`GIT_DIR=/tmp/perry-regexp-lane.git ./remote.sh '<command>'`.
Nothing was built or tested on the Mac.

Fresh main and candidate compiler/runtime/stdlib bundles are respectively
`/tmp/regexp-main-1557-libs` and `/tmp/regexp-1557-candidate-libs`.
Their identical release package sets and explicit Rust flags built successfully
in 3m 59s and 4m 19s. This comparison freezes the main reference above; older
comparisons remain in [final-results.md](final-results.md).

## Attribution and final implementation

The [original real-input profiles](results.md) established the cause before
editing: the active engine was Perex with **no construction cache**. Every
constructor reparsed and rebuilt its program. The old SipHash/fallback cache
analysis described an inactive implementation, not the measured execution path.

| Path | Original CPU attribution |
|---|---|
| 14,116-character emoji constructor | Parser cursor 32.13%, sequence parsing 18.28%, escapes 17.83%, character classes 6.73%, emission 6.27%. |
| Reused emoji test on ASCII | About 77% revalidating immutable program words, ranges and repeats. Main now supplies an in-cell validation witness. |
| Plain stripAnsi | Property/reflective reads, flag getters, closure dispatch, exception traps and runtime handles dominate. Validation is 2.12%; no-match replacement already avoided result arrays. |

The final change adds a 512-entry, 32 MiB program/source payload LRU. A source
identity plus canonical flags hit reads no pattern bytes. Independently
allocated equal strings use a content hash plus exact comparison. Eviction
removes individual entries; lookbehind and backreferences use the same engine
and cache. Source/program cells are mutable GC roots, with the identity index
rewritten after movement. Original source strings are sealed against in-place
append. Receiver state and `lastIndex` remain independent.

Compilation stays eager, preserving constructor syntax errors and avoiding a
deferred compile on first test. Main's immutable program witnesses avoid repeat
validation. Its newer cross-call non-ASCII cursor table is preserved, including
heap-generation invalidation. Optional scratch hints from an earlier experiment
were removed; scratch buffers remain owned and budgeted per operation.

Canonical builtin guards avoid generic property dispatch for ordinary tests
and empty replacements. Overrides, descriptors, coercions and altered
prototypes retain their ordinary observable behavior. Numeric `lastIndex`
avoids a coercion trap. Plain no-match replacement returns the input; actual
matches build output from spans without exec arrays or capture strings.

A diagnostic snapshot after the constructor loop reports **three compilations,
200 identity hits and 14,277 source bytes hashed**, exactly the three initial
source lengths. The 200 repeated emoji constructions add no compilation or
pattern hashing. Diagnostic attribution itself inspects pattern bytes; the
timings below were collected with diagnostics disabled.

The prior final profiles show the remaining tier: native dispatch, search
setup, the VM, exception traps, handle management, guards and scratch allocation.
Repeated parsing and program validation no longer dominate. See
[the prior profile breakdown](final-results.md#attribution-and-implementation)
and [ownership notes](../../docs/src/internals/regexp-program-cache.md).

## Real OpenCode dependency probe

OpenCode v1.18.30 supplies emoji-regex 10.6.0, string-width 7.2.0,
strip-ansi 7.1.2 and get-east-asian-width 1.6.0. The pattern is the real
14,116-character source. Perry reports 10 modules; both arms compile with
`--no-auto-optimize --debug-symbols`. Bun is 1.3.14.

Twelve balanced rounds on CPU 15 cover all six before/after/Bun orders.
Native builds use one executable pathname/inode, copied outside timing.
All checksums and non-timing output agree. The original `all 1` counts are
200 constructions, 2,000 tests/replacements, 200 segmentations and 90 width
calls, checksum 162430. Medians below are wall microseconds per operation,
including the first operation.

| Operation | Main | Candidate | Bun |
|---|---:|---:|---:|
| Emoji construction | 608.531 | 4.566 | 3.490 |
| Reused emoji test, ASCII | 2.266 | 0.882 | 0.675 |
| Reused emoji test, rocket | 3.449 | 2.162 | 0.579 |
| stripAnsi, plain help line | 12.668 | 0.593 | 0.590 |
| stripAnsi, ANSI-colored text | 26.248 | 4.747 | 0.490 |
| Default_Ignorable test | 1.779 | 0.390 | 0.284 |
| Intl.Segmenter | 46.684 | 51.747 | 20.761 |
| eastAsianWidth | 0.788 | 0.872 | 1.161 |
| stringWidth | 41813.031 | 394.400 | 49.468 |

Construction improves **133×**, plain stripAnsi **21×**, and stringWidth
**106×**. Construction, the issue's reused ASCII test, and plain stripAnsi
meet their 20/2/1 µs targets. The rocket test is **2.163 µs** in this loaded
run and **2.167 µs** at 200,000 calls (main 3.258, Bun 0.061).
It exceeds the 2 µs target here; the prior integration measured 1.269 µs.
Actual ANSI matches remain above 1 µs. These are recorded limits, not omitted
samples. [Raw probe samples](current-probe-samples.json).

This shared host had another large native OpenCode compilation running.
Absolute times and identical-binary controls vary substantially. Our own
correctness jobs started only after these timing runs. No full OpenCode binary
was rebuilt or timed for this lane; dependency measurements do not establish
the issue's under-one-second full startup expectation.

## Existing benchmark controls

All 12 existing `benchmarks/app-patterns/kernels/*.ts` compile against each
matching bundle and match Node 26.5.1. Twelve balanced four-arm rounds follow
one warmup: two identical-copy labels per build, 24 samples/build. Values are
pooled median child user CPU milliseconds. A/A compares the two identical-copy
label medians within each build. Wall samples are retained in
[the raw artifact](current-app-samples.json).

| Kernel | Main ms | Candidate ms | Change | Main A/A | Candidate A/A |
|---|---:|---:|---:|---:|---:|
| batch | 47.604 | 48.038 | 0.91% | 4.29% | 5.42% |
| buffer_transcode | 73.163 | 71.779 | -1.89% | 1.38% | 4.78% |
| date_format_parse | 46.109 | 46.189 | 0.17% | -0.25% | -0.87% |
| json_parse_1mb | 81.657 | 79.575 | -2.55% | -0.38% | 5.48% |
| json_stringify_1mb | 95.008 | 94.004 | -1.06% | -10.41% | -33.91% |
| map_1m | 229.503 | 229.816 | 0.14% | -0.09% | -4.15% |
| object_deep_clone | 29.527 | 28.119 | -4.77% | -8.86% | -0.52% |
| promise_all_chains | 157.143 | 157.506 | 0.23% | -14.39% | -8.06% |
| regex_replace | 3412.720 | 2889.472 | -15.33% | -7.21% | 5.05% |
| string_concat_csv | 47.117 | 47.617 | 1.06% | 1.68% | 2.28% |
| string_split_map_join | 280.455 | 299.048 | 6.63% | -11.69% | -1.71% |
| string_template_interp | 74.776 | 76.552 | 2.37% | -2.45% | 5.80% |

The existing regex replacement kernel improves **15.33%**. Large A/A spreads,
including 33.91% in JSON stringify, prevent interpreting every small sweep
delta as a code effect. The 100-round JSON parse control (200 samples/build)
is **86.078 → 85.172 ms, -1.05%**; the previous integration's loaded JSON
slowdown does not reproduce against this main/build pair.
[Raw JSON control](current-json-control-samples.json).

Longer controls use 100 balanced four-arm rounds, 200 samples/build. The
intervals resample balanced four-round blocks 5,000 times, with seed 10179.

| Kernel | Main ms | Candidate ms | Change | 95% block-bootstrap interval |
|---|---:|---:|---:|---|
| json_parse_1mb | 86.077 | 85.172 | -1.05% | -1.72% to 0.43% |
| json_stringify_1mb | 136.675 | 137.079 | 0.30% | -2.62% to 1.85% |
| string_concat_csv | 46.861 | 48.161 | 2.78% | 1.56% to 3.58% |
| string_split_map_join | 291.086 | 293.124 | 0.70% | -1.63% to 7.98% |

JSON stringify and split/join do not show a resolved slowdown beyond these
controls. CSV concatenation retains **+2.78%**, with the interval above zero.
The strict no-regression requirement is **not met**, so the PR stays in draft.
The earlier JSON issue has not simply been dismissed as noise, nor does its
absence in the current comparison establish that every other benchmark is
unaffected. No padding, unrelated string optimization or benchmark alteration
was added to conceal this result.
[Other raw controls](current-other-control-samples.json).

## Correctness

- `cargo test -p perry-runtime --lib regex`: **103 passed** (9.78s).
- Upstream cursor-position slice: **three passed** (0.82s).
- Full runtime lib: **3,746 passed, four ignored, one failed** (208.04s).
  `native_stack::tests::stack_top_respects_custom_thread_stack_sizes` fails
  identically on freshly built main; seven other native-stack tests pass there.
- Regex parity against Node 26.5.1: **25 passed**, zero differences, compile
  failures, crashes or skips (5m 53s including the HTTP extension rebuild).
- Full pinned RegExp/Annex B Test262: **1,786 passed, 129 runtime failures,
  zero output differences, zero compile failures, seven skipped**. There are
  1,915 judged cases and 210 negative agreements. All 129 failure-name/category
  pairs match fresh main exactly, with none added or removed.
  [Comparison artifact](current-test262-comparison.json).
- Formatting, file-size cap, registration (298 files/four registries), GC-root
  inventory (1,420 holders/155 scanners) and Node-version consistency: pass.
- `pre-tag-check.sh --quick`: only public benchmark evidence freshness fails.
  Fresh main reproduces that failure with exit 2; the candidate quick suite
  returns exit 1 and lists that check alone.

Inspection with `rg -n regex .github/workflows/test.yml` finds the reduced-feature
comment, not a separately named regex/Test262 job. The runtime/parity coverage
and full RegExp plus Annex B corpus above are therefore run explicitly.
Test262 is pinned at `4249661388e5d3f92a85186213da140a6481490f`, and Node
26.5.1 matches the repository oracle. The benchmark regression and unmeasured
full startup remain explicit acceptance gaps.

## Exact host commands

All commands below were invoked through the remote wrapper; `taskset` places
timings on CPU 15. Each release bundle contains the compiler and both static
archives from one build. Baseline and candidate builds ran sequentially in the
same lane, checking out the named source before each build.

```sh
export RUSTFLAGS="-C force-unwind-tables=yes -C force-frame-pointers=yes"
export PATH=/tmp/regexp-oracle/node-v26.5.1-linux-x64/bin:$PATH
# For main 64f5249ac0 and candidate snapshot c03b3348e0:
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
# Copy perry, libperry_runtime.a and libperry_stdlib.a from the target release
# directory into /tmp/regexp-main-1557-libs or /tmp/regexp-1557-candidate-libs.

python3 benchmarks/regexp-construction/prepare.py /tmp/regexp-probe-1557.ts
PERRY_RUNTIME_DIR=/tmp/regexp-main-1557-libs /tmp/regexp-main-1557-libs/perry \
  compile /tmp/regexp-probe-1557.ts -o /tmp/regexp-probe-main-1557 \
  --no-auto-optimize --debug-symbols
PERRY_RUNTIME_DIR=/tmp/regexp-1557-candidate-libs /tmp/regexp-1557-candidate-libs/perry \
  compile /tmp/regexp-probe-1557.ts -o /tmp/regexp-probe-candidate-1557 \
  --no-auto-optimize --debug-symbols
taskset -c 15 python3 benchmarks/regexp-construction/measure.py \
  --before /tmp/regexp-probe-main-1557 --after /tmp/regexp-probe-candidate-1557 \
  --source /tmp/regexp-probe-1557.ts --bun /root/claude-opencode/bun/bin/bun \
  --output /tmp/regexp-1557-probe.json --runs 12 \
  --case all:1 --case test-emoji:100
taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-main-1557-libs --after /tmp/regexp-1557-candidate-libs \
  --output /tmp/regexp-1557-apps --runs 12 --paired-controls
taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-main-1557-libs --after /tmp/regexp-1557-candidate-libs \
  --output /tmp/regexp-1557-json-control --runs 100 --paired-controls \
  --filter json_parse_1mb
for kernel in json_stringify_1mb string_concat_csv string_split_map_join; do
  taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
    --before /tmp/regexp-main-1557-libs --after /tmp/regexp-1557-candidate-libs \
    --output "/tmp/regexp-1557-control-$kernel" --runs 100 --paired-controls \
    --filter "$kernel"
done

export RUST_TEST_THREADS=1 CARGO_BUILD_JOBS=6
taskset -c 9-14 cargo test -p perry-runtime --lib regex
taskset -c 9-14 cargo test -p perry-runtime --lib perex_position_hint
taskset -c 9-14 cargo test -p perry-runtime --lib
# In main 64f5249ac0:
taskset -c 9-14 cargo test -p perry-runtime --lib native_stack
python3 benchmarks/ci_public_baseline_check.py

# Match the candidate compiler's build stamp before building its HTTP extension:
git checkout --detach c03b3348e0905d2ef7d9ffe628508f3b0f216689
PERRY_SKIP_BUILD=1 PERRY_BIN=/tmp/regexp-1557-candidate-libs/perry \
  PERRY_RUNTIME_DIR=/tmp/regexp-1557-candidate-libs taskset -c 9-14 \
  ./run_parity_tests.sh --filter regex
PERRY_RUNTIME_DIR=/tmp/regexp-main-1557-libs taskset -c 0-3 \
  python3 scripts/test262_subset.py --root /tmp/regexp-test262 \
  --dir built-ins/RegExp annexB/built-ins/RegExp --all-features --jobs 4 \
  --perry-bin /tmp/regexp-main-1557-libs/perry \
  --report /tmp/regexp-test262-main-1557.json --sample-cap 100000
PERRY_RUNTIME_DIR=/tmp/regexp-1557-candidate-libs taskset -c 4-6,8 \
  python3 scripts/test262_subset.py --root /tmp/regexp-test262 \
  --dir built-ins/RegExp annexB/built-ins/RegExp --all-features --jobs 4 \
  --perry-bin /tmp/regexp-1557-candidate-libs/perry \
  --report /tmp/regexp-test262-candidate-1557.json --sample-cap 100000
# Restore the final source snapshot before lint.
cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 scripts/check_test_registration.py
python3 scripts/gc_runtime_root_holders.py
python3 scripts/check_node_version_consistency.py --list
./scripts/pre-tag-check.sh --quick
PERRY_REGEX_DIAG=1 taskset -c 15 /tmp/regexp-probe-candidate-1557 all 1
```

The original profiling commands and their pre-edit attribution are retained in
[results.md](results.md#reproduction-commands). The benchmark drivers now default to 12
rounds, which is divisible by both schedule lengths. Explicit `--runs 12`
and `--runs 100 --paired-controls` were used for the recorded measurements.
