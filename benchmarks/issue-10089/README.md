# DataView numeric setter fast path (#10089)

`binary-dataview-set.ts` and `binary-dataview-get.ts` are the unchanged,
standalone reproducers embedded in issue #10089. Their seeded setup, minimum
200 ms/five-run warmup, seven samples of at least 20 ms, median calculation,
fresh-input policy, and per-invocation checksum checks are preserved.

Measured on 2026-09-12 on Linux x86_64 with an AMD Ryzen 7 7700X (16 logical
CPUs), Node 26.5.1, LLVM 22.1.8, and the pinned nightly-2026-08-20 Rust
toolchain. Before is current main `50e08e91dd6a54d9d9210c43a5d36c86d880d144`
(Perry 0.5.1539); after is this change on the same commit. Both builds used
`--release --locked`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, the same package
set, and matching compiler/runtime/stdlib archives. Compilation used
`--no-auto-optimize` with `PERRY_RUNTIME_DIR` pointing at those archives. Runs
were serialized. Source and executable hashes are in `artifacts.json`.

## Current-main comparison

Times are median milliseconds per workload invocation. The setter workload
writes and immediately reads every value; the getter control only reads. Every
Perry checksum matched Node at every size.

| n | Before Perry setter | After Perry setter | After Node setter | Before ratio | After ratio | After getter ratio |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.006030 | 0.005387 | 0.000269 | 22.50x | 20.03x | 11.52x |
| 1,000 | 0.058436 | 0.052222 | 0.002510 | 23.36x | 20.80x | 12.46x |
| 10,000 | 0.581095 | 0.518971 | 0.024820 | 23.51x | 20.91x | 12.35x |
| 100,000 | 5.730139 | 5.192406 | 0.247603 | 23.26x | 20.97x | 12.33x |
| 1,000,000 | 58.591890 | 51.767301 | 2.471560 | 23.70x | 20.95x | 12.31x |

The Perry setter workload improves by 9.4–11.6% and is 1.72–1.76 times the
getter control, rather than the issue's original 6.68 times at 1M. The
remaining difference includes a second accessor per iteration and numeric
wrapping/storage; the getter baseline itself remains out of scope. Raw results
are `before.json` and `after.json`.

Least-squares log(time)/log(n) slopes remain linear: before setter Perry 0.997,
Node 0.992; after setter Perry 0.996, Node 0.992. After getter slopes are Perry
0.999 and Node 0.994.

## Pre-change attribution

The two original costs were measured independently on the issue's pinned
revision `9495bfc95e2afcfb5a7cb535e440e61ec0722cb1`, before changing behavior.
The no-handle-scope patch is a timing diagnostic only. The no-propagation patch
redirects the write to the canonical shared backing before removing the old
reverse-table propagation, preserving checksums and modeling the storage design
that subsequently landed in #10071.

| n | Historical baseline | No handle scope | Direct backing/no propagation | Getter control |
|---:|---:|---:|---:|---:|
| 100 | 0.006275 | 0.006034 (-3.8%) | 0.005392 (-14.1%) | 0.002951 |
| 1,000 | 0.062615 | 0.059653 (-4.7%) | 0.053234 (-15.0%) | 0.029187 |
| 10,000 | 0.618650 | 0.592610 (-4.2%) | 0.529090 (-14.5%) | 0.290650 |
| 100,000 | 6.176727 | 5.909852 (-4.3%) | 5.307551 (-14.1%) | 2.906022 |
| 1,000,000 | 51.473179 | 48.430062 (-5.9%) | 51.179968 (-0.6%) | 29.168526 |

Across the stable 100–100k range, propagation/view-table work was consistently
the dominant extra setter cost, 3.2–3.6 times the handle-scope cost. The 1M
propagation row reproduces the threshold anomaly already called out in the
issue and is not used to reverse the four-size attribution. Getter controls at
100k were 2.9060, 2.8957, and 2.9139 ms across the three builds.

#10071 landed at `1ae0f84497` between the pinned and current-main measurements.
It made views share canonical backing storage and deleted the reverse-table
propagation call. This change removes the remaining setter-only work: DataView
construction caches the stable backing byte pointer, and calls whose offset and
value are already Numbers neither probe `VIEW_REGISTRY` nor publish a transient
GC handle. Coercing and BigInt calls retain the handle and reload the receiver
after user code.

Raw attribution data and the exact diagnostic patches are committed alongside
the current-main comparison.

## Reproduce

From the checkout being measured:

```sh
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo build --release --locked \
  -p perry -p perry-runtime-static -p perry-stdlib-static
export PERRY_RUNTIME_DIR="$PWD/target/release"
target/release/perry compile \
  benchmarks/issue-10089/binary-dataview-set.ts \
  --no-auto-optimize -o /tmp/binary-dataview-set
target/release/perry compile \
  benchmarks/issue-10089/binary-dataview-get.ts \
  --no-auto-optimize -o /tmp/binary-dataview-get
python3 benchmarks/issue-10089/measure.py \
  --set-app /tmp/binary-dataview-set \
  --get-app /tmp/binary-dataview-get \
  --node node \
  --output /tmp/results.json
```

Do not run timed processes or builds concurrently. Rebuild all three packages
when changing checkouts.

## Semantic and GC validation

`test_gap_10089_dataview_setter_fast_path.ts` covers both byte orders and all
numeric kinds, byte-level endianness, out-of-range and negative offsets,
ToNumber strings/objects/abrupt completion, BigInt/Number type errors,
ToBigInt-before-bounds ordering, detach during coercion, BigInt round trips, and
DataView/Uint8Array writes in both directions. Its coercion callback allocates,
calls `gc()`, and runs under forced evacuation, from-space protection, and
evacuation verification. Its output matches Node 26.5.1 byte-for-byte.

The focused runtime test also checks that a windowed DataView stores the exact
backing pointer in its private payload and writes through it.
