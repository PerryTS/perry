# RegExp construction cache: final Perex 0.1.4 verification

Historical verification against Perry 0.5.1554. See
[current-results.md](current-results.md) for the subsequent 0.5.1557 integration.

The final runtime is `7be32ca293`, integrating main
`5d3bf85f92632cd026ec2695fd1a6929ffb56d72` (Perry 0.5.1554 / Perex 0.1.4).
Its production runtime sources match preserved build snapshot
`b2599a6d60beeae085822d1ccdc359c8f058c8d8`.
A subsequent test-only assertion formatting fix changes no runtime code.
There is one construction cache, upstream's in-cell program-validation witness,
guarded builtin dispatch, and span-based empty replacement. Compilation stays
eager; scratch buffers remain operation-owned. The optional scratch-hint
experiment was removed after an existing-benchmark regression.

Every build, test, probe, and profile ran on Linux through
`GIT_DIR=/tmp/perry-regexp-lane.git ./remote.sh '<command>'`; none ran on the Mac.
Fresh main and final use matching compiler/runtime/stdlib bundles with identical
explicit release flags. The main bundle is `/tmp/regexp-main-1554-libs`
(built from detached `/tmp/regexp-main-1554`); final is
`/tmp/regexp-cache-final-libs`. Their builds took 5m 01s and 5m 40s.

## Attribution and implementation

The original attribution was established before runtime edits; see
[results.md](results.md). The active implementation already used Perex and had
**no construction cache**. Neither SipHash cache misses nor an uncached fallback
engine explained the repeated compilation. Lookbehind and backreferences use
the same engine and cache.

| Real input | Original sampled CPU attribution |
|---|---|
| 14,116-character emoji construction | Parser cursor 32.13%, sequence 18.28%, escapes 17.83%, character classes 6.73%, emission 6.27%. Every construction reparsed and rebuilt the program. |
| Reused emoji test on ASCII | About 77% validating immutable words (`Program::from_words`, ranges and repeats). Upstream's program witness now handles this across calls. |
| Plain stripAnsi | Generic property/reflective reads, flag getters, closure dispatch, exception traps and runtime handles dominate; program validation contributes 2.12%. No-match replacement already avoided result arrays. |

The cache bounds source/program payload to 32 MiB and 512 entries and evicts
individual LRU entries. Source-identity plus canonical-flag hits read no pattern
bytes; independent equal strings use hashing followed by exact comparison.
Mutable roots retain source/program cells and rewrite identity keys after
movement. Every RegExp retains its own original source/flags and `lastIndex`.
Source strings are marked shared against in-place append.

Builtin guards check receiver metadata, expandos, prototype shape, current
`exec` value, flag getter identities and symbol epochs. Observable overrides,
coercions and descriptors retain the ordinary path. Empty replacement returns
the original input on no match and builds matched output from spans without
exec arrays/capture strings. Numeric `lastIndex` avoids a coercion trap.

Final diagnostics count **three compilations, 200 identity hits and 14,277
source bytes hashed**, exactly the three initial source lengths. The 200 reused
emoji constructions add no source hashing or compilation. The short final
construction profile encountered heavy kernel page migration/compaction;
its percentage breakdown is not treated as representative. The remaining
user work is object publication, flags, handle management, barriers and a
small identity/flags hash.

Final ASCII-test samples attribute 6.46% to native method dispatch, 5.64% to
search setup, 4.60% to primitive dispatch, 4.31% to the VM, 4.11% to exception
traps and 2.86% to scratch zeroing. Plain replacement shows search setup
9.35%, VM 6.99%, exception traps 6.36%, replacement entry 6.08%, plus handles,
guards and scratch allocation. Repeated program parsing/validation no longer
dominates either path. These boolean/no-match cases allocate no exec result
object; actual ANSI matches additionally assemble output spans.

## Real-package probe

The probe resolves OpenCode v1.18.30's installed emoji-regex 10.6.0,
string-width 7.2.0, strip-ansi 7.1.2 and get-east-asian-width 1.6.0.
Perry reports 10 modules, versus the issue's earlier nine-module count.
Bun is 1.3.14. Both native arms use `--no-auto-optimize`.

Twelve balanced rounds on CPU 15 cover all six before/after/Bun orders.
Both native builds execute at one fixed pathname/inode; copying is outside
the timed region. Every checksum and non-timing output agrees. `all 1` uses
the original counts: 200 constructions, 2,000 tests/replacements, 200
segmentations and 90 string-width calls; checksum 162430. These medians include
the first operation and are wall microseconds per operation.

| Operation | Main | Final | Bun 1.3.14 |
|---|---:|---:|---:|
| Emoji construction | 428.249 | 3.339 | 2.853 |
| Reused emoji test, ASCII | 1.403 | 0.512 | 0.663 |
| Reused emoji test, rocket | 2.136 | 1.260 | 0.493 |
| stripAnsi, plain help line | 7.747 | 0.346 | 0.417 |
| stripAnsi, ANSI-colored text | 16.556 | 2.650 | 0.390 |
| Default_Ignorable test | 1.106 | 0.227 | 0.258 |
| Intl.Segmenter | 30.584 | 29.325 | 14.742 |
| eastAsianWidth | 0.488 | 0.461 | 1.005 |
| stringWidth | 30120.383 | 241.578 | 29.013 |

Construction improves **128×**, plain stripAnsi **22×**, and stringWidth
**125×**. Construction meets 20 µs, both reused tests meet 2 µs, and plain
stripAnsi meets 1 µs. ANSI text with actual matches still costs 2.650 µs.
The longer isolated rocket test (scale 100, 200,000 calls) measures
**2.154 → 1.269 µs**, Bun 0.047 µs.
[Raw samples](final-probe-samples.json).

Warm Bun remains much faster at matching. These dependency measurements do
not establish a final startup time for a rebuilt full `opencode --help`.
This is a shared host; absolute times change with other builds. Our own
concurrent checks excluded CPU 15 and its SMT sibling 7.

## Existing benchmarks and the rejected experiment

All 12 `benchmarks/app-patterns/kernels/*.ts` are compiled with each matching
bundle and checked against Node 26.5.1. Twelve balanced four-arm rounds follow
one warmup: two identical-copy labels per build, 24 samples/build. Timings are
median child user CPU milliseconds; wall samples are retained too.

| Kernel | Main ms | Final ms | Change | Main A/A | Final A/A |
|---|---:|---:|---:|---:|---:|
| batch | 42.725 | 42.415 | -0.73% | -3.19% | +0.89% |
| buffer_transcode | 68.292 | 68.619 | +0.48% | -0.59% | -1.77% |
| date_format_parse | 45.191 | 45.535 | +0.76% | -2.21% | +2.07% |
| json_parse_1mb | 83.011 | 84.247 | +1.49% | +0.42% | -0.94% |
| json_stringify_1mb | 131.093 | 131.135 | +0.03% | -3.55% | -0.04% |
| map_1m | 310.660 | 327.919 | +5.56% | -2.70% | -1.61% |
| object_deep_clone | 28.301 | 28.728 | +1.51% | +1.33% | -5.84% |
| promise_all_chains | 141.036 | 122.427 | -13.19% | -17.87% | -17.15% |
| regex_replace | 3091.335 | 2776.635 | -10.18% | +1.27% | -3.63% |
| string_concat_csv | 45.956 | 44.423 | -3.34% | +6.04% | -4.82% |
| string_split_map_join | 273.491 | 251.919 | -7.89% | -2.40% | -8.30% |
| string_template_interp | 65.850 | 63.677 | -3.30% | +4.76% | +4.34% |

The A/A columns compare identical-copy label medians within each build.
This loaded-host sweep contains large control spreads; its small deltas are
not a universal no-regression guarantee. The existing regex replacement kernel
improves 10.18%. [All raw app samples](final-app-samples.json).

The longer 100-round loaded controls reduce the Map delta to **+0.33%**
(286.647 → 287.602 ms; 95% block-bootstrap interval -0.33% to +2.54%).
JSON remains **+1.93%** (85.122 → 86.761 ms; interval +0.58% to +2.86%),
despite the earlier quieter control below. The strict no-regression requirement
therefore remains unmet, and the PR is held in draft. No padding or unrelated
JSON optimization was added to hide this result.
Intervals resample balanced four-round blocks 5,000 times, seed 10179;
the raw data, rather than the interval alone, is the evidence.
[Loaded control samples](final-loaded-control-samples.json).

A 100-round JSON control (200 samples/build) measures
**49.961 → 49.817 ms**, a -0.29% change. The individual identical-copy label
medians are 49.539 / 50.172 ms for main and 49.994 / 49.624 ms for final.
[Raw JSON control](final-json-control-samples.json).

An optional scalar-hint experiment (`bc9212fcc8`) reduced emoji scratch
growth from 2.000 to 0.500/search and isolated rocket-test time by 28%.
Its unit, moving-GC, parity and Test262 results passed. However, JSON parsing
slowed by 2.59% in 100 balanced rounds, and about 1.6% on another CPU core.
GC diagnostics showed identical collection/object/byte counts and no RegExp
operations. Disassembly showed the same hot parser loop bytes moved from
`0x254194` to `0x256ca4`, crossing a 64-byte instruction-cache line.
That suggests a layout effect, but it does not erase the measured regression.
The experiment was removed; the smaller implementation still meets the regex
targets. [Experiment data](hint-experiment-samples.json).

## Correctness and required checks

- Regex runtime slice: **103 passed** (8.43s).
- Full runtime lib: **3,720 passed, four ignored, one failed** (164.80s).
  The failure is `native_stack::tests::stack_top_respects_custom_thread_stack_sizes`;
  fresh main reproduces it with the same flags (seven other native-stack tests pass).
- Regex parity against Node 26.5.1: **25 passed**, zero differences, compile
  failures, crashes or skips (5m 15s, including the HTTP extension rebuild).
- Full pinned Test262: **1,786 passed, 129 runtime failures, zero output
  differences, zero compile failures, seven skipped**; 1,915 judged and
  210 negative agreements. All 129 failure-name/category pairs match fresh main.
  [Comparison artifact](final-test262-comparison.json).
- Final formatting, file-size cap, registration (296 files/four registries),
  GC-root inventory (1,413 holders/155 scanners) and Node-version checks: pass.
  The first post-revert fmt check found one assertion layout; it was corrected
  and the checks rerun successfully.
- Final `pre-tag-check.sh --quick`: only public benchmark evidence freshness
  fails. The same failure was reproduced on freshly built main; its committed
  public artifact was already stale.
- Strict performance gate: **not yet met**. The final loaded JSON control
  retains a small slowdown, detailed below; this is not a regression-free verdict.

The current `test.yml` has no separately named regex/Test262 job
(`rg -n regex .github/workflows/test.yml` finds only a feature comment).
Its relevant coverage comes from runtime and parity suites. The focused parity
slice and full RegExp plus Annex B Test262 corpus are run explicitly here.
Test262 is pinned at `4249661388e5d3f92a85186213da140a6481490f`;
Node 26.5.1 matches `.node-version`.

## Exact host commands

All commands below ran through the remote wrapper. Each source tree built all
three matching release artifacts; no unrelated prebuilt compiler was used.

```sh
export RUSTFLAGS="-C force-unwind-tables=yes -C force-frame-pointers=yes"
export PATH=/tmp/regexp-oracle/node-v26.5.1-linux-x64/bin:$PATH
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib regex
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib
# In freshly built detached main:
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib native_stack
python3 benchmarks/ci_public_baseline_check.py

cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 scripts/check_test_registration.py
python3 scripts/gc_runtime_root_holders.py
python3 scripts/check_node_version_consistency.py --list
./scripts/pre-tag-check.sh --quick

python3 benchmarks/regexp-construction/prepare.py /tmp/regexp-probe.ts
PERRY_RUNTIME_DIR=/tmp/regexp-main-1554-libs /tmp/regexp-main-1554-libs/perry \
  compile /tmp/regexp-probe.ts --no-auto-optimize --debug-symbols \
  -o /tmp/regexp-probe-main-1554
PERRY_RUNTIME_DIR=/tmp/regexp-cache-final-libs /tmp/regexp-cache-final-libs/perry \
  compile /tmp/regexp-probe.ts --no-auto-optimize --debug-symbols \
  -o /tmp/regexp-probe-cache-final
taskset -c 15 python3 benchmarks/regexp-construction/measure.py \
  --before /tmp/regexp-probe-main-1554 --after /tmp/regexp-probe-cache-final \
  --source /tmp/regexp-probe.ts --bun /root/claude-opencode/bun/bin/bun \
  --output /tmp/regexp-nohint-target-recheck.json --runs 12 \
  --case all:1 --case test-emoji:100
taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-main-1554-libs --after /tmp/regexp-cache-final-libs \
  --output /tmp/regexp-app-nohint-final --runs 12 --paired-controls
taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-main-1554-libs --after /tmp/regexp-cache-final-libs \
  --output /tmp/regexp-nohint-json-control --runs 100 --paired-controls \
  --filter json_parse_1mb

for kernel in map_1m json_parse_1mb; do
  taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
    --before /tmp/regexp-main-1554-libs --after /tmp/regexp-cache-final-libs \
    --output "/tmp/regexp-nohint-final-control-$kernel" --runs 100 \
    --paired-controls --filter "$kernel"
done

# Match the preserved compiler's build ID before the HTTP extension build:
git checkout --detach b2599a6d60beeae085822d1ccdc359c8f058c8d8
PERRY_SKIP_BUILD=1 PERRY_BIN=/tmp/regexp-cache-final-libs/perry \
  PERRY_RUNTIME_DIR=/tmp/regexp-cache-final-libs taskset -c 0-6,8-14 \
  ./run_parity_tests.sh --filter regex
PERRY_RUNTIME_DIR=/tmp/regexp-main-1554-libs taskset -c 0-7 \
  python3 scripts/test262_subset.py --root /tmp/regexp-test262 \
  --dir built-ins/RegExp annexB/built-ins/RegExp --all-features --jobs 4 \
  --perry-bin /tmp/regexp-main-1554-libs/perry \
  --report /tmp/regexp-test262-main-1554.json --sample-cap 100000
PERRY_RUNTIME_DIR=/tmp/regexp-cache-final-libs taskset -c 0-6,8-14 \
  python3 scripts/test262_subset.py --root /tmp/regexp-test262 \
  --dir built-ins/RegExp annexB/built-ins/RegExp --all-features --jobs 8 \
  --perry-bin /tmp/regexp-cache-final-libs/perry \
  --report /tmp/regexp-test262-nohint-final.json --sample-cap 100000

PERRY_REGEX_DIAG=1 taskset -c 15 /tmp/regexp-probe-cache-final all 1
for mode in construct test-ascii stripAnsi; do
  taskset -c 15 perf record -q -F 999 -g --call-graph dwarf \
    -o "/tmp/regexp-nohint-final-$mode.perf" \
    /tmp/regexp-probe-cache-final "$mode" 1000
  perf report --stdio --no-children -g none \
    -i "/tmp/regexp-nohint-final-$mode.perf" --percent-limit 1 -F overhead,symbol
done
```

The benchmark tools now reject non-positive/non-integer iteration controls.
Fourteen invalid driver cases and 15 invalid direct scales were checked; valid
output agreed across Perry, Node and Bun. The direct positive-scale guard was
added outside the timed loops after the preserved native timing samples.
