# Repeated string trimming (#10054)

Current main at `603b074ace01464bc66fc07cc8d532f26ccf5a0f` reproduced the issue before implementation. Both original standalone sources are copied unchanged from #10054. The fixed measurements use this PR’s runtime patch over that revision; `fixed.json` records the exact `git diff HEAD -- crates/` SHA-256 and compiler/runtime archive hashes.

## Change and limits

Trim scans the requested whitespace edges and subtracts their UTF-16 lengths from the source header. A cold result is copied through a rooted source handle, re-read after allocation. An unchanged managed string can be shared safely; foreign strings retain the copying contract.

For retained results of at least 256 bytes, a single-entry per-thread cache reuses the last source/result pair and trim mode. Both pointers are strong, rewritable GC roots. A 32 MiB combined capacity/header budget bounds retention, including spare source capacity; the GC’s own allocation metadata is additional. Replacement drops the previous pair. Cached sources and returned aliases are marked shared to prevent in-place append from changing them.

This removes full-interior work from repeated trims of a reused immutable receiver. **Cold trims, cache evictions, results below 256 bytes, and inputs exceeding the retention budget still copy the retained bytes in O(n).** Malformed WTF-8 with ambiguous reverse boundaries uses the historical bounded forward scan. The flat StringHeader/codegen/FFI ABI is preserved; general substring views remain separate representation work relevant to #10061.

## Method

- Apple M1 Max, 10 logical cores, macOS 26.5, arm64; Node v26.5.1; Perry 0.5.1532.
- Compiler and both matching static archives built together with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 cargo build --release --locked -j 4 -p perry -p perry-runtime-static -p perry-stdlib-static`.
- Every fixture compiled with `--no-auto-optimize` and an explicit `PERRY_RUNTIME_DIR`.
- Sizes 100, 1,000, 10,000, 100,000, 1,000,000, in that order; Node then Perry, sequentially. Original warmup (at least 200 ms and five runs), seven samples of at least 20 ms, and 60-second per-process timeout retained.
- All five generated-source hashes match the before run. All 25 before/after checksum pairs match Node, including both original Unicode and ASCII workloads through one million tokens.
- This is a contended shared host. Other work ran during the sweeps, so exact constant-factor comparisons are not controlled. The Node medians also vary substantially. The robust result is the removal of interior-length scaling for the repeated-input trim work; repeat on an idle host for precise ratios.

Load averages: original baseline start/end `[66.587890625, 63.22119140625, 58.33251953125]` / `[31.310546875, 52.17822265625, 54.625]`; alternating baseline `[23.7412109375, 47.94580078125, 52.9814453125]` / `[27.30078125, 47.900390625, 52.90576171875]`; fixed sweep `[64.9306640625, 64.12451171875, 62.9033203125]` / `[55.21435546875, 63.736328125, 63.16015625]`.

## Original ASCII workload

All times below are median milliseconds per 16-trim workload invocation. Each ratio uses the matching Node measurement from that sweep.

| n | Node before | Perry before | Before / Node | Node after | Perry after | After / Node |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.014689 | 0.028460 | 1.94× | 0.034925 | 0.034734 | 0.99× |
| 1,000 | 0.013383 | 0.173485 | 12.96× | 0.032738 | 0.034824 | 1.06× |
| 10,000 | 0.014670 | 1.686392 | 114.96× | 0.028254 | 0.035096 | 1.24× |
| 100,000 | 0.014566 | 16.730479 | 1148.62× | 0.049022 | 0.028340 | 0.58× |
| 1,000,000 | 0.014982 | 171.586958 | 11453.21× | 0.043383 | 0.037424 | 0.86× |

Perry fitted exponent: **0.954 → -0.002**.

The fixed exponent meets the issue’s ≤0.25 target.

## Original Unicode workload

| n | Node before | Perry before | Before / Node | Node after | Perry after | After / Node |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.014747 | 0.236148 | 16.01× | 0.067698 | 0.537645 | 7.94× |
| 1,000 | 0.013858 | 2.280791 | 164.58× | 0.149693 | 7.471167 | 49.91× |
| 10,000 | 0.013893 | 22.489500 | 1618.79× | 0.171185 | 87.803917 | 512.92× |
| 100,000 | 0.014943 | 206.249875 | 13802.09× | 0.081558 | 1096.120542 | 13439.78× |
| 1,000,000 | 0.014706 | 3910.500792 | 265912.70× | 0.131619 | 2035.471833 | 15464.83× |

Perry fitted exponent: **1.039 → 0.932**.

## Separate Unicode costs

`unicode-trim-only` replaces `hashString(input.trim())` with `input.trim().length`. `unicode-checksum-only` trims the input once in setup, outside the timers, then hashes that retained input 16 times. They retain the original timing driver. Both controls have matching Node checksums at every size.

| n | Trim before | Trim after | Indexed checksum before | Indexed checksum after |
|---:|---:|---:|---:|---:|
| 100 | 0.050435 | 0.000146 | 0.207776 | 0.251263 |
| 1,000 | 0.412874 | 0.000207 | 1.977652 | 3.120101 |
| 10,000 | 5.184156 | 0.000134 | 19.563500 | 33.208917 |
| 100,000 | 52.680416 | 0.000121 | 176.898625 | 347.406042 |
| 1,000,000 | 945.056333 | 0.000143 | 1866.769917 | 2324.477791 |

Trim-only fitted exponent: 1.065 → -0.025. The indexed checksum remains linear (0.998); that code is unchanged and is tracked by #10055. These are separate runs under variable host load, so their times should not be subtracted or expected to add exactly to the combined workload. The remaining Unicode ratio is not attributed to trim.

## Cache-miss control

`string-trim-alternating-ascii` alternates two distinct prebuilt inputs, one with an extra retained `x`. This deliberately replaces the single-entry cache on each trim and exposes the remaining copy cost. Setup remains outside the timers.

| n | Node before | Perry before | Before / Node | Node after | Perry after | After / Node |
|---:|---:|---:|---:|---:|---:|---:|
| 100 | 0.041673 | 0.066735 | 1.60× | 0.018384 | 0.019752 | 1.07× |
| 1,000 | 0.108987 | 1.060643 | 9.73× | 0.021098 | 0.031742 | 1.50× |
| 10,000 | 0.040129 | 4.153041 | 103.49× | 0.020626 | 0.121460 | 5.89× |
| 100,000 | 0.035648 | 44.763833 | 1255.71× | 0.016727 | 1.020254 | 60.99× |
| 1,000,000 | 0.049176 | 328.138917 | 6672.69× | 0.015629 | 10.768861 | 689.03× |

Perry fitted exponent: **0.901 → 0.698**.

The cache-miss workload still scales with the retained payload. Boundary scanning and known UTF-16 lengths reduce its cost, but the representation-imposed copy remains.

## Reproduce

Run from the checkout whose matching compiler and archives were built:

```sh
python3 benchmarks/string_trim/measure.py before
python3 benchmarks/string_trim/measure.py after
```

Use `--runtime-dir` and `--output-dir` to select other matching build artifacts and keep each checkout’s results. `--only` accepts workload names. The script writes binaries, exact generated sources, compile logs and JSON measurements below `target/string-trim` by default. The original baseline used the same generated source bytes and timing loop; its two sweeps are preserved separately in `baseline.json` and `baseline-cold.json`.

## Validation

- Runtime unit suite: 3,534 passed, 4 ignored, zero failures, single-threaded. Includes all new trim tests, existing malformed guard-page coverage, and string-copy moving-GC tests. The new cache test asserts that both cached strings actually relocate and that a subsequent trim reuses the relocated result.
- New compiled trim fixture matches Node byte-for-byte.
- GC root-holder audit and test registration checks pass.
- Broad `pre-tag-check.sh --quick` passes except for the pre-existing public benchmark freshness failure. All public source/harness fingerprint inputs are byte-identical to the base commit; published source fingerprint `bec8afb6e384640aa9090036e3ad393ac564750a083f6e6c485804b6784c38b8` differs from the base/current `9507434be47f7bb383c30810bdc5d66d9be65290da529f38b7473860ea98f75e`.
