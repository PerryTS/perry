# Historical verification against main df886c6445

This intermediate revision predates upstream cross-call program witnesses and
Perex 0.1.4. Its binding cache has since been removed.
See [final-results.md](final-results.md) for the final implementation and evidence.

The original measurements in [results.md](results.md) apply to commit
`62bddb2929` against `9b911855f8ca`. Before publishing the final revision, main
landed per-operation binding reuse, forward split search, and an unlimited
work policy. Merge `a33b82498a` integrates main `df886c644562dd745170b2d40c9fb7d3261112e3`.
Per-operation reuse now retains the same shared validated owner as standalone
calls; the current-receiver/current-program checks and subject reuse remain.
The main GC witness additionally checks that independent receivers incur no
second validation charge, across actual moving collections.

Both fresh compiler/runtime/stdlib bundles use release builds and the same
explicit upstream flags:

```sh
export RUSTFLAGS="-C force-unwind-tables=yes -C force-frame-pointers=yes"
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
```

The main bundle is `/tmp/regexp-current-main-libs`; the candidate bundle is
`/tmp/regexp-merged-libs`. The candidate build took 4m 56s. The lane's Mac Cargo
config remains untouched; the flags above are supplied on the build host.
All commands in this report run through `GIT_DIR=/tmp/perry-regexp-lane.git
./remote.sh '<command>'`.

## Fresh real-input probe

Seven alternating rounds on CPU 15, identical counts, and checked checksums.
Values are median wall µs/operation. The original-count `all 1` probe uses 200
constructions, 2,000 tests/replacements, 200 segmentations, and 90 width calls.
Raw data: [merged-probe-samples.json](merged-probe-samples.json).

| Operation | Current main | Merged candidate | Bun 1.3.14 |
|---|---:|---:|---:|
| emoji construction | 685.625 | 4.752 | 3.516 |
| reused emoji test, ASCII | 11.238 | 0.925 | 0.714 |
| reused emoji test, rocket | 12.438 | 1.717 | 0.568 |
| stripAnsi, plain help line | 14.184 | 0.628 | 0.603 |
| stripAnsi, ANSI-colored text | 29.730 | 3.652 | 0.482 |
| Default_Ignorable test | 1.958 | 0.409 | 0.304 |
| Intl.Segmenter, mixed run | 47.398 | 51.590 | 18.885 |
| eastAsianWidth, mixed run | 0.859 | 0.912 | 1.293 |
| stringWidth | 43262.696 | 402.426 | 54.055 |

Construction improves 144×, plain stripAnsi 23×, and stringWidth 108×. Both
reused tests meet 2 µs, construction meets 20 µs, and plain stripAnsi meets
1 µs. Replacement with actual ANSI matches still takes 3.652 µs.

Longer isolated cases use 2,000 constructions and 200,000 calls otherwise:

| Operation | Current main | Merged candidate | Bun 1.3.14 |
|---|---:|---:|---:|
| emoji construction | 590.414 | 0.637 | 0.713 |
| reused emoji test, ASCII | 9.845 | 0.894 | 0.053 |
| reused emoji test, rocket | 12.554 | 1.721 | 0.068 |
| stripAnsi, plain help line | 13.988 | 0.631 | 0.067 |
| stripAnsi, ANSI-colored text | 22.999 | 3.472 | 0.128 |

The first compilation remains eager and contributes to the short construction
run. The longer run amortizes it. These values do not measure complete OpenCode
startup. Isolated controls for the slower mixed-run non-regex cases are below.

## Existing app-pattern kernels

All 12 kernels match Node output under both builds. Fifteen alternating rounds,
one warmup, a fixed executable pathname/inode, CPU 15; medians in child user CPU
milliseconds. Raw data: [merged-app-samples.json](merged-app-samples.json).

| Kernel | Current main | Merged candidate | Change |
|---|---:|---:|---:|
| batch | 50.424 | 49.647 | -1.54% |
| buffer_transcode | 74.968 | 74.733 | -0.31% |
| date_format_parse | 47.963 | 47.347 | -1.28% |
| json_parse_1mb | 88.707 | 86.485 | -2.50% |
| json_stringify_1mb | 136.164 | 138.734 | 1.89% |
| map_1m | 337.453 | 333.186 | -1.26% |
| object_deep_clone | 29.958 | 28.791 | -3.90% |
| promise_all_chains | 189.206 | 187.887 | -0.70% |
| regex_replace | 5392.096 | 4472.262 | -17.06% |
| string_concat_csv | 47.338 | 48.372 | 2.18% |
| string_split_map_join | 302.636 | 303.716 | 0.36% |
| string_template_interp | 78.603 | 81.695 | 3.93% |

Existing regex replacement improves 17.1%. The positive deltas are retained
here; follow-up 101-round comparisons and A/A controls are reported below.


## Correctness and repository checks

- Regex unit slice: 103 passed.
- Full runtime lib: 3,708 passed, one failed, four ignored, 119.61 seconds.
  The sole failure is `native_stack::tests::stack_top_respects_custom_thread_stack_sizes`.
  It reproduces on current main with identical flags (seven pass, one fail).
- Regex parity slice, Node 26.5.1: 25 pass, zero differences, zero compile
  failures, zero crashes/skips. The HTTP-extension case now passes too.
- Both full RegExp Test262 runs: 1,786 pass, 129 runtime failures, seven skips,
  zero differences/compile failures. All 129 failing `(test, bucket)` pairs are
  identical; no new or resolved failures.
- Formatting, the 2,000-line Rust cap, registration (294 files/four registries),
  and GC root inventory (1,411 holders/155 scanners) pass.
- `pre-tag-check.sh --quick`: all checks pass except public benchmark evidence
  freshness, also reproduced on the untouched baseline. GC store and address
  classification audits pass.

```sh
export PATH=/tmp/regexp-oracle/node-v26.5.1-linux-x64/bin:$PATH
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib regex
RUST_TEST_THREADS=1 cargo test -p perry-runtime --lib
RUST_TEST_THREADS=1 cargo test --manifest-path /tmp/regexp-current-main/Cargo.toml \
  -p perry-runtime --lib native_stack
PERRY_SKIP_BUILD=1 PERRY_BIN=/tmp/regexp-merged-libs/perry \
  PERRY_RUNTIME_DIR=/tmp/regexp-merged-libs ./run_parity_tests.sh --filter regex
cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 scripts/check_test_registration.py
python3 scripts/gc_runtime_root_holders.py
./scripts/pre-tag-check.sh --quick
PERRY_RUNTIME_DIR=/tmp/regexp-current-main-libs python3 scripts/test262_subset.py \
  --root /tmp/regexp-test262 --dir built-ins/RegExp annexB/built-ins/RegExp \
  --all-features --jobs 4 --perry-bin /tmp/regexp-current-main-libs/perry \
  --report /tmp/regexp-test262-current-main.json --sample-cap 100000
PERRY_RUNTIME_DIR=/tmp/regexp-merged-libs python3 scripts/test262_subset.py \
  --root /tmp/regexp-test262 --dir built-ins/RegExp annexB/built-ins/RegExp \
  --all-features --jobs 4 --perry-bin /tmp/regexp-merged-libs/perry \
  --report /tmp/regexp-test262-merged.json --sample-cap 100000
```

For the HTTP extension rebuild, the host checked out the exact sync commit
used to build the preserved compiler, `125cb677813e7c5325e6c122583c3e8910b6cdcc`,
before invoking parity. This prevents the earlier archive/compiler build-ID
mismatch; the source is the merged candidate above.
