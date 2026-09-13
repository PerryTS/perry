# Issue #10179: measurement and verification record

This is the historical Perex 0.1.0 lane record, including a binding cache later
removed in favor of upstream program witnesses. See [final-results.md](final-results.md)
for the final Perex 0.1.4 implementation and comparison.

Linux x86-64 build host, 2026-09-13. Baseline: Perry
`9b911855f8ca877ee439d5ddc416b27d4b7b018f`, Perex 0.1.0. Both compilers
use release builds and matching runtime/stdlib archives. All native compilation,
tests and probes ran on the host through `./remote.sh`; none ran on the Mac.
The oracle is Node 26.5.1 (`.node-version`), and the timing comparison uses Bun
1.3.14 and the actual installed OpenCode v1.18.30 dependencies.

## Attribution before changing the code

This checkout has already replaced the old standard/fallback split with Perex.
There was **no construction cache** on the active path. `js_regexp_new`, literal
sites, constructor calls, and `RegExp.prototype.compile` all eagerly compiled
through `perex_construct`. The issue was not a hash miss, eviction, or an
uncached fallback engine. Lookbehind and backreferences use the same engine.

`perf record -F 499 -g --call-graph dwarf` on the unmodified probe found:

| Operation | Where sampled CPU went |
|---|---|
| 14,116-character emoji construction | Parser cursor 32.13%, sequence 18.28%, escape parsing 17.83%, class parsing 6.73%, program emission 6.27%; remaining parser routines dominate the rest. No cache lookup or fallback engine. |
| Reused emoji `.test("a")` | `Program::from_words` 66.69%, range validation 8.13%, repeat validation 2.67%: about 77% repeatedly validating the same immutable program. |
| Plain-text `stripAnsi` | Object field reads 6.73%, reflective Get 5.09%, runtime handles 4.95%, field lookup tail 4.60%, exception traps 4.39%, accessor lookup 4.38%, UTF-8 validation 3.82%, RegExp property reads 2.86%, closure dispatch 2.76%, program validation 2.12%. Generic `@@replace`, `exec`, and eight flag reads dominate. This no-match case did not allocate match results. |

The changes were measured incrementally: construction sharing alone brought
construction to 0.588 µs, but reused ASCII tests still took 9.964 µs; binding
reuse brought tests to 2.262 µs; guarded builtin dispatch and empty replacement
brought tests to 1.115 µs and plain `stripAnsi` to 0.568 µs. These exploratory
samples used `all 10` and were collected at different host loads, so they are
attribution evidence, not the controlled final comparison.

That revision kept eager syntax checking, two bounded LRU caches (512
entries/32 MiB each), per-object state, and operation-owned scratch. Source
identity hits avoid hashing the pattern; independently allocated equal sources
use a hash and exact comparison. Perex bindings retain their original immutable
owner and are rewritten on GC movement. Cache scanners register on first use.

After optimization, construction samples move to runtime handles (17.49%),
constructor setup (12.84%), the JS factory closure (11.26%), publication and GC
barriers. The remaining SipHash work (3.45%) hashes short identity/flag keys;
pattern bytes are only hashed on source-identity misses. Reused tests move to
host search setup (8.48%), native method dispatch (7.12%), VM matching (4.75%),
and primitive dispatch (4.47%). Plain replacement moves to host search setup
(10.64%), VM matching (8.13%), the String replacement boundary (6.68%), subject
binding (6.39%), and exception traps (5.94%). It returns the original string on
no match and uses spans rather than exec arrays when deleting matches.

## Final probe comparison

Seven alternating rounds, pinned to CPU 15, with identical inputs, iteration
counts, and checked checksums. Values are median wall microseconds per operation.
The host is shared, so raw samples are included in [probe-samples.json](probe-samples.json).
The original-count `all 1` run uses 200 constructions, 2,000 tests/replacements,
200 segmentations, and 90 string-width calls; checksum 162430.

| Operation | Perry before | Perry after | Bun 1.3.14 |
|---|---:|---:|---:|
| emoji construction | 884.630 | 4.930 | 3.879 |
| reused emoji test, ASCII | 11.928 | 0.929 | 0.737 |
| reused emoji test, rocket | 13.487 | 1.725 | 0.588 |
| stripAnsi, plain help line | 15.784 | 0.614 | 0.690 |
| stripAnsi, ANSI-colored text | 29.906 | 4.786 | 0.426 |
| Default_Ignorable test | 1.962 | 0.376 | 0.318 |
| Intl.Segmenter | 50.742 | 64.026 | 21.888 |
| eastAsianWidth | 0.867 | 0.791 | 1.372 |
| stringWidth | 47926.129 | 459.889 | 51.902 |

Construction is 179× faster, plain stripAnsi 26× faster, and stringWidth 104×
faster in this run. Construction, both reused tests, and plain stripAnsi meet
the requested 20/2/1 µs targets. ANSI text with matches still takes 4.786 µs:
decoding, repeated searches, and output construction remain. The mixed-run
Segmenter result is slower; an isolated follow-up is reported below.

Separate longer cases amortize initial work and Bun warmup:

| Operation | Iterations | Perry before | Perry after | Bun 1.3.14 |
|---|---:|---:|---:|---:|
| emoji construction | 2000 | 744.492 | 2.558 | 0.743 |
| reused emoji test, ASCII | 200000 | 12.190 | 0.966 | 0.079 |
| reused emoji test, rocket | 200000 | 13.167 | 1.704 | 0.079 |
| stripAnsi, plain help line | 200000 | 15.206 | 0.528 | 0.076 |
| stripAnsi, ANSI-colored text | 200000 | 31.724 | 4.504 | 0.153 |

The warm results retain a substantial Bun gap despite meeting the targets.
These are timed-loop results, not a complete CLI startup measurement.

## Existing benchmark comparison

All 12 existing app-pattern kernels match pinned Node output under both builds.
The following 15-round medians use child user CPU milliseconds, one warmup,
alternating execution order, CPU 15, and one fixed executable path for both
builds. Raw samples: [app-samples.json](app-samples.json).

| Existing kernel | Before (ms) | After (ms) | Change |
|---|---:|---:|---:|
| batch | 49.759 | 51.183 | 2.86% |
| buffer_transcode | 76.919 | 75.305 | -2.10% |
| date_format_parse | 46.199 | 46.363 | 0.35% |
| json_parse_1mb | 100.600 | 98.845 | -1.74% |
| json_stringify_1mb | 136.806 | 138.325 | 1.11% |
| map_1m | 327.807 | 321.354 | -1.97% |
| object_deep_clone | 27.981 | 28.352 | 1.33% |
| promise_all_chains | 164.130 | 171.369 | 4.41% |
| regex_replace | 6153.973 | 5354.053 | -13.00% |
| string_concat_csv | 42.662 | 43.877 | 2.85% |
| string_split_map_join | 308.978 | 304.255 | -1.53% |
| string_template_interp | 78.797 | 77.954 | -1.07% |

The regex replacement kernel improves by 13.0%. Small positive deltas require
controls on this busy host; they are not silently rounded to zero. An initial
101-round JSON comparison appeared about 1.7% slower, but an A/A comparison
using byte-identical binaries reproduced 1.7% (SHA-256
`59adac3c48986eff98f1c2a4fa507285ab793b487da0d34375f547e607ed770b`).
The harness now copies each build to one fixed pathname/inode outside the
timed window, removing argv[0]/execPath as a variable. Under this harness the
101-round JSON A/A medians are 132.288/133.561 ms (+0.96%), while A/B is
128.572/129.423 ms (+0.66%). This does not establish a JSON regression.
Additional controls are recorded below and in [controls-samples.json](controls-samples.json).

The 101-round controls for the other initially slower kernels give:

| Kernel | A/A before / after (ms) | A/A change | A/B before / after (ms) | A/B change |
|---|---:|---:|---:|---:|
| batch | 27.035 / 27.964 | 3.44% | 33.977 / 31.231 | -8.08% |
| promise_all_chains | 122.030 / 124.991 | 2.43% | 126.570 / 128.014 | 1.14% |
| string_concat_csv | 46.047 / 44.907 | -2.48% | 43.790 / 43.530 | -0.59% |

The isolated `segment 100` follow-up (20,000 iterations, seven rounds,
matching checksum 1340000) measures 44.991 µs before, 43.909 µs after, and
4.773 µs in Bun. The slower mixed-run Segmenter observation does not reproduce
in isolation. These controls establish no repeatable slowdown in the kernels
checked; they cannot certify zero slowdown for every workload on a shared host.

## Reproduction commands

Each command below ran on the Linux host, invoked from the Mac as
`GIT_DIR=/tmp/perry-regexp-lane.git ./remote.sh '<command>'`. The isolated Git
directory is a lane workaround for read-only parent worktree metadata.

```sh
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
export PERRY_RUNTIME_DIR="$CARGO_TARGET_DIR/release"
python3 benchmarks/regexp-construction/prepare.py /tmp/regexp-probe.ts
"$CARGO_TARGET_DIR/release/perry" compile /tmp/regexp-probe.ts \
  -o /tmp/regexp-probe-after --no-auto-optimize --debug-symbols
/root/claude-opencode/bun/bin/bun /tmp/regexp-probe.ts all 1
PERRY_REGEX_DIAG=1 /tmp/regexp-probe-after all 1
perf record -q -F 499 -g --call-graph dwarf -o /tmp/regexp-construct-before.perf \
  /tmp/regexp-probe-before-symbols construct 100
perf report --stdio --no-children -g none -i /tmp/regexp-construct-before.perf \
  --percent-limit 1 -F overhead,symbol
taskset -c 15 python3 benchmarks/regexp-construction/measure.py \
  --before /tmp/regexp-probe-before --after /tmp/regexp-probe-lazy \
  --source /tmp/regexp-probe.ts --bun /root/claude-opencode/bun/bin/bun \
  --output /tmp/regexp-probe-final.json --runs 7
taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-baseline-libs --after /tmp/regexp-lazy-libs \
  --output /tmp/regexp-app-fixed --runs 15
for kernel in json_stringify_1mb batch promise_all_chains string_concat_csv; do
  taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
    --before /tmp/regexp-baseline-libs --after /tmp/regexp-baseline-libs \
    --output /tmp/regexp-aa-fixed-$kernel --runs 101 --filter $kernel
  taskset -c 15 python3 benchmarks/regexp-construction/compare.py \
    --before /tmp/regexp-baseline-libs --after /tmp/regexp-lazy-libs \
    --output /tmp/regexp-app-fixed-$kernel --runs 101 --filter $kernel
done
taskset -c 15 python3 benchmarks/regexp-construction/measure.py \
  --before /tmp/regexp-probe-before --after /tmp/regexp-probe-lazy \
  --source /tmp/regexp-probe.ts --bun /root/claude-opencode/bun/bin/bun \
  --output /tmp/regexp-segment-final.json --runs 7 --case segment:100
```

The same perf commands were run for `test-ascii` and `stripAnsi`. Final samples
used `-F 999`, `--percent-limit 2`, and scale 10000. Timings exclude diagnostics
and perf. The diagnostics snapshot showed three compiled programs, 200 identity
hits, zero content hits, and only 14,277 bytes hashed after 203 constructions;
the emoji program was validated once, with repeated canonical test dispatches.

## Verification

```sh
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib regex
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib
cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 scripts/check_test_registration.py
python3 scripts/gc_runtime_root_holders.py
./scripts/pre-tag-check.sh --quick
export PATH=/tmp/regexp-oracle/node-v26.5.1-linux-x64/bin:$PATH
PERRY_RUNTIME_DIR=/tmp/regexp-lazy-libs /tmp/regexp-lazy-libs/perry compile \
  test-files/test_gap_10179_regexp_cache.ts --no-auto-optimize \
  -o /tmp/regexp-parity-final
node --experimental-strip-types test-files/test_gap_10179_regexp_cache.ts \
  > /tmp/regexp-parity-final-node.txt
/tmp/regexp-parity-final > /tmp/regexp-parity-final-perry.txt
diff -u /tmp/regexp-parity-final-node.txt /tmp/regexp-parity-final-perry.txt
PERRY_SKIP_BUILD=1 PERRY_BIN=/tmp/regexp-final-libs/perry \
  PERRY_RUNTIME_DIR=/tmp/regexp-final-libs ./run_parity_tests.sh --filter regex
grep -rn regex .github/workflows/test.yml
PERRY_RUNTIME_DIR=/tmp/regexp-final-libs python3 scripts/test262_subset.py \
  --root /tmp/regexp-test262 --dir built-ins/RegExp annexB/built-ins/RegExp \
  --all-features --jobs 4 --perry-bin /tmp/regexp-final-libs/perry \
  --report /tmp/regexp-test262-after.json --sample-cap 100000
```

The final regex unit slice passes 102 tests. The full runtime lib run reports
3,686 passed, one failed, and four ignored. Formatting, file size, test
registration, and GC root inventory pass. The full quick gate additionally
checks GC stores and address classifications; its baseline failure is below.

The workflow contains no dedicated regex/Test262 job; the committed Test262
radar was run over both RegExp subtrees, pinned at
`4249661388e5d3f92a85186213da140a6481490f`. Baseline and candidate both report
1,786 passes, 129 runtime failures, zero output differences, zero compile
failures, and seven skips (1,915 judged, 93.3%). The complete sets of failing
`(test, bucket)` pairs are identical.

The full Test262 run preceded the last registration-only refinement (register
cache scanners on first use). Matching code is identical; the final full
runtime/GC tests and standalone Node fixture exercise the final registration.
`/tmp/regexp-lazy-libs` and `/tmp/regexp-probe-lazy` are the final snapshots;
`/tmp/regexp-final-libs` is the earlier snapshot used by the full parity radar.

The regex parity runner reports 24 matches, no runtime differences or crashes,
and one HTTP-extension compile failure. Its automatic extension rebuild used a
different sync-commit build ID from the preserved compiler; the linker correctly
rejected it. The standalone #10179 fixture also matches Node byte for byte,
with 48 matching output lines, covering source, flags, independent lastIndex,
sticky/global behavior, compile,
invalid patterns, read-only lastIndex, prototype/own overrides, and Unicode.

The full runtime suite's sole failure is
`native_stack::tests::stack_top_respects_custom_thread_stack_sizes`. It also
fails in an untouched baseline worktree with
`RUST_TEST_THREADS=1 cargo test --manifest-path /tmp/regexp-baseline-tree/Cargo.toml -p perry-runtime --lib native_stack`
(7 pass, 1 fail). Disabling glibc's stack cache did not resolve it.
`pre-tag-check.sh --quick` passes every check except published benchmark
freshness; that exact failure also reproduces on the untouched baseline.

This is a standalone change against the lane's pinned base. Open PR #10183
also changes cross-call binding using Perex 0.1.3 and overlaps this area; merge
order needs review. These measurements cover the real dependency probe, not a
newly rebuilt full OpenCode executable, so they do not establish its final
`--help` startup time.
