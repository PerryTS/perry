# Unicode indexed-access regression (#10055)

Measured on 2026-09-11, Apple M1 Max / arm64, macOS 26.5, Node v26.5.1,
Rust nightly-2026-08-20. The before build is current-main revision
`603b074ace01464bc66fc07cc8d532f26ccf5a0f`; the after build adds this PR's runtime
changes. Both builds use the same package set and release settings:

```sh
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked \
  -p perry -p perry-runtime-static -p perry-stdlib-static
PERRY_RUNTIME_DIR="$PWD/target/release" target/release/perry compile CASE.ts \
  --no-auto-optimize -o APP
```

The four indexing workloads are the unchanged complete sources in
[issue #10055](https://github.com/PerryTS/perry/issues/10055); trim uses the
unchanged source in [issue #10054](https://github.com/PerryTS/perry/issues/10054).
Use the issues' driver: at least 200 ms and five warmup runs, seven samples of
at least 20 ms measured work each, then the median per invocation. Setup is
outside the timer. Each process has a 60-second limit; after a timeout the
larger Perry size is skipped. Every completed checksum matches Node.

Runs were serialized, but other builds shared this host. Load and process wall
times are recorded in [results.jsonl](results.jsonl); constant-factor differences
are subject to this contention. Compiler/archive and source hashes are in
[artifacts.json](artifacts.json). No version metadata was bumped.

## Same-size timings

Times are milliseconds per invocation. Unicode `n` repeats `ä中😀Ö`, so the
largest input contains five million UTF-16 units. ASCII repeats `aBcD`.

| Workload | n | Before ms | After ms | Node after ms | Checksum |
|---|---:|---:|---:|---:|---:|
| char-code-at-unicode | 100 | 0.200894 | 0.056010 | 0.033121 | 319467163 |
| char-code-at-unicode | 1,000 | 26.354292 | 0.659186 | 0.378739 | 431622199 |
| char-code-at-unicode | 10,000 | 2889.565334 | 5.388000 | 3.122719 | 36132863 |
| char-code-at-unicode | 100,000 | TIMEOUT | 53.879334 | 30.159500 | 49951631 |
| char-code-at-unicode | 1,000,000 | skipped after timeout | 569.439875 | 376.385500 | 481167302 |
| index-bracket-unicode | 100 | 0.239787 | 0.063120 | 0.032486 | 319467163 |
| index-bracket-unicode | 1,000 | 21.795709 | 0.542565 | 0.372876 | 431622199 |
| index-bracket-unicode | 10,000 | 2305.712583 | 6.266302 | 3.199137 | 36132863 |
| index-bracket-unicode | 100,000 | TIMEOUT | 47.097333 | 34.163667 | 49951631 |
| index-bracket-unicode | 1,000,000 | skipped after timeout | 763.104291 | 313.539292 | 481167302 |
| char-code-at-ascii | 100 | 0.008931 | 0.016492 | 0.032008 | 464151292 |
| char-code-at-ascii | 1,000 | 0.094285 | 0.218839 | 0.165308 | 710929850 |
| char-code-at-ascii | 10,000 | 1.077884 | 2.031650 | 2.631313 | 535454277 |
| char-code-at-ascii | 100,000 | 11.170167 | 25.715042 | 22.728292 | 35382078 |
| char-code-at-ascii | 1,000,000 | 107.527208 | 222.769875 | 300.137250 | 153135489 |
| index-bracket-ascii | 100 | 0.014128 | 0.029684 | 0.029850 | 464151292 |
| index-bracket-ascii | 1,000 | 0.197181 | 2.039900 | 0.328588 | 710929850 |
| index-bracket-ascii | 10,000 | 1.609375 | 4.379000 | 5.090604 | 535454277 |
| index-bracket-ascii | 100,000 | 17.254500 | 30.125625 | 24.644000 | 35382078 |
| index-bracket-ascii | 1,000,000 | 174.843959 | 365.966875 | 299.907667 | 153135489 |

Log/log exponents over the same three large completed after sizes (10,000,
100,000, 1,000,000):

- `string-char-code-at-unicode`: **1.012**.
- `string-index-bracket-unicode`: **1.043**.
- `string-char-code-at-ascii`: **1.020**.
- `string-index-bracket-ascii`: **0.961**.

Both Unicode exponents must be at most 1.25. The deterministic unit test also
counts actual decoder calls through `charCodeAt` and requires linear work.

At n=1,000,000, `string-char-code-at-ascii` changes by **+107.2%** in elapsed time. At n=1,000,000, `string-index-bracket-ascii` changes by **+109.3%** in elapsed time. The ASCII direct-byte lookup branches remain intact.

The separate sweep phases also slowed Node's ASCII controls by 132.3% and
110.5%, respectively. To check this contention effect, the preserved before
and after executables were rerun immediately in five alternating-order pairs
at n=1,000,000. Median of the five per-process medians:

| ASCII workload | Before ms | After ms | Change |
|---|---:|---:|---:|
| char-code-at-ascii | 101.504708 | 99.922583 | -1.56% |
| index-bracket-ascii | 149.060292 | 145.558625 | -2.35% |

No ASCII slowdown is observed in this paired check; small differences remain
subject to host noise. Raw pairs: [ascii-paired.jsonl](ascii-paired.jsonl).

## Shared checksum-cost controls

These include the bounded `hashString` helper's roughly 32 `charCodeAt`
lookups. Improvements here include the shared indexing fix; they do not isolate
the cost of trim, repeat, or padding. The producing builtins are unchanged.
The repeat/padStart companion cases use the same driver and checksum helper,
with these setup/run bodies (the output name is updated accordingly):

```ts
// string-repeat-unicode
function setup(n: number): number { return n; }
function run(input: number): number { return hashString("ä中😀Ö".repeat(input)); }
// string-pad-start-unicode
function setup(n: number): number { return n; }
function run(input: number): number { return hashString("x".padStart(input * 5 + 1, "ä中😀Ö")); }
```

| Workload | n | Before ms | After ms | Before / Node | After / Node |
|---|---:|---:|---:|---:|---:|
| trim-unicode | 10,000 | 22.277333 | 12.820208 | 1404.82× | 298.50× |
| trim-unicode | 100,000 | 294.341916 | 158.062917 | 16106.49× | 3011.70× |
| trim-unicode | 1,000,000 | 3633.753417 | 744.497375 | 212059.09× | 38951.15× |
| repeat-unicode | 10,000 | 3.688408 | 0.173978 | 266.33× | 7.76× |
| repeat-unicode | 100,000 | 24.759312 | 1.575067 | 205.56× | 26.85× |
| repeat-unicode | 1,000,000 | 239.610959 | 16.837979 | 478.18× | 45.72× |
| pad-start-unicode | 10,000 | 3.062786 | 0.397263 | 159.11× | 31.31× |
| pad-start-unicode | 100,000 | 35.156708 | 3.067208 | 291.62× | 49.57× |
| pad-start-unicode | 1,000,000 | 306.407500 | 32.624333 | 456.02× | 85.89× |

## Correctness and lifetime coverage

The TypeScript regression compares forward, reverse, repeated random, interleaved,
and growing-string accesses with explicit expected code units. It covers short
and heap strings, BMP characters, both emoji halves, lone surrogates, and bounds.
The new `at()` test also exposed its older replacement-character encoder; `at()`
now uses the same WTF-8 single-code-unit builder as `charAt()`.

Rust tests assert actual relocation with preserved checkpoints, death pruning
on copying and full mark/sweep cycles, scanner registration, cache eviction,
length-change invalidation, bounded random rescans, and guard-page safety for
truncated WTF-8 tails. The cache contains four weak identities with Rust-owned
offsets; it keeps no interior pointers and does not retain otherwise dead strings.
The index is built lazily, with a checkpoint approximately every 128 payload
bytes; hot forward access uses a cursor and other seeks use binary search.

Validation commands (run runtime tests with one thread):

```sh
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 RUST_TEST_THREADS=1 \
  cargo test --release --locked -p perry-runtime --lib utf16_index
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 RUST_TEST_THREADS=1 \
  cargo test --release --locked -p perry-runtime --lib 'string::'
```

Compiled Node-parity fixtures: `test_gap_unicode_indexing_10055`,
`test_gap_7592_char_code_at_inline`, `test_gap_9431_array_from_lone_surrogate`,
and `test_parity_string_append_surrogate_repair`. The new fixture also runs
with `PERRY_GC_SCHEDULE_SEED=10055`, `PERRY_GC_SCHEDULE_RATE=1`,
`PERRY_GC_SCHEDULE_ALLOC_KB=4`, `PERRY_GC_PROTECT_FROMSPACE=1`,
`PERRY_GC_VERIFY_EVACUATION=1`, and `PERRY_GC_DIAG=1`.

Final stress verdict: `[gc-schedule] forced_collections=693 copying_minors=693 moved_objects=12681 loop_polls=83872`.
