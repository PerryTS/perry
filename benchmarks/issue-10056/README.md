# Shared Buffer/Uint8Array subarrays (#10056)

`subarray.ts` is the unchanged standalone source embedded in
[issue #10056](https://github.com/PerryTS/perry/issues/10056). Its seeded setup,
200 ms / five-run warmup, seven samples of at least 20 ms, median calculation,
and per-invocation checksum checks are preserved.

Measured on 2026-09-11, Windows 11 Pro x86_64, Ryzen 5 7640HS (12 logical CPUs),
Node 26.5.1, LLVM 22.1.8, and the repository's pinned Rust nightly-2026-08-20.
Before: current main `603b074ace01464bc66fc07cc8d532f26ccf5a0f` (Perry 0.5.1532).
After: this PR's runtime sources, with no version change. Both builds used
`--release --locked`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, the same package
set, and matching compiler/runtime/stdlib archives. Compilation used
`--no-auto-optimize` with `PERRY_RUNTIME_DIR` pointing at those archives.
Artifact and source hashes are in `artifacts.json`.

## Unchanged workload

Times below are median milliseconds per invocation. The driver runs engines
and sizes serially, with a 60-second process timeout covering setup, warmup,
sampling and shutdown. It stops an engine after a timeout, as in the issue.

| n | Before Perry | After Perry ms | After Node ms | Matching checksum |
|---:|---:|---:|---:|---:|
| 100 | TIMEOUT | 0.097039 | 0.002338 | 18622 |
| 1,000 | skipped | 0.946259 | 0.022904 | 630318 |
| 10,000 | skipped | 8.568000 | 0.228144 | 51281607 |
| 100,000 | skipped | 101.492100 | 2.257533 | 12816444 |
| 1,000,000 | skipped | 1184.189700 | 22.595200 | 127857054 |

Least-squares log(time)/log(n) slopes over all five completed after sizes:
**Perry 1.020, Node 0.996**. A before slope for the unchanged workload is
**unavailable**, because even its first Perry size timed out on this host.
The original macOS timings in the issue were not remeasured here.
Raw results, including the before Node sweep, are `before.json` and `after.json`.

Node's before medians were 0.002411 ms at n=100 and 23.595500 ms at
n=1,000,000, versus 0.002338 and 22.595200 ms after. Timed
processes and builds ran serially, but host load was not controlled. Use the
completed workload, checksum agreement, allocation proof and near-linear
scaling as the evidence; absolute timings remain specific to this host.

## Separate single-invocation diagnostic

`single-run.ts` uses the same seeded setup and `run(input)` body, but calls it
once in a fresh process without warmup/sampling. This is a diagnostic reduction,
not a substitute for the unchanged workload above. It separates view creation
from the accumulated cleanup that prevents the before workload from completing.

| n | Before Perry ms | After Perry ms |
|---:|---:|---:|
| 100 | 0.3965 | 0.1743 |
| 1,000 | 1.4043 | 0.7932 |
| 10,000 | 55.0554 | 7.0505 |
| 100,000 | 3643.3899 | 77.2641 |
| 1,000,000 | not run | 1037.0412 |

Every completed checksum agrees with the unchanged workload. On the common
sizes [100, 1,000, 10,000, 100,000], diagnostic slopes are **1.348 before** and
**0.889 after**. Single-call overhead and host contention affect these slopes;
the warmed after sweep and the allocation regression are stronger evidence of
the intended complexity change. Raw data: `kernel-before.json`, `kernel-after.json`.

## Reproduce

From the checkout being measured (POSIX shell; on Windows use the equivalent
PowerShell environment assignments and `.exe` executable names):

```sh
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo build --release --locked -p perry -p perry-runtime-static -p perry-stdlib-static
export PERRY_RUNTIME_DIR="$PWD/target/release"
target/release/perry compile benchmarks/issue-10056/subarray.ts --no-auto-optimize -o /tmp/subarray
python benchmarks/issue-10056/measure.py /tmp/subarray --output /tmp/results.json
```

For the diagnostic, compile `single-run.ts` with the same flags and invoke the
result with each size as its final argument. Do not run timed processes or
builds concurrently. Rebuild all three packages when changing checkouts.

## Semantic and GC validation

- Seven runtime regressions cover header-only allocations, shared native/indexed
  and numeric writes, overlapping/nested views, clamped/negative/empty bounds,
  backing identity, overlapping copy/set, detach, and dead-index cleanup.
- A GC regression roots only a nursery holder of a nested view, asserts that the
  holder actually moves, then performs a full collection. The backing and cached
  ArrayBuffer survive, the intermediate receiver dies, and dropping the holder
  lets the complete backing/identity cycle be collected.
- Runtime unit suite: **3,443 passed, 4 ignored**, with one Windows malloc-trim
  telemetry expectation excluded after independently reproducing its failure
  (`emergency_full_trace_is_excluded_from_ordinary_pause_stats`: expected
  `unsupported`, received `executed`).
- **61 FFI tests passed**, including shared/nested byte-window reads through
  the stable extern bridge. Release checks pass for all changed native consumer
  crates (zlib, fastify, ethers, HTTP, net and updater).
- **18 selected Node parity fixtures passed**, including both new buffer
  regressions. Six fixtures with try/catch used `PERRY_RS4GC=0` for the documented
  Windows WinEH limitation (#7354); both new fixtures use the default GC.
  `PERRY_DISABLE_WELL_KNOWN=1` selects the rebuilt stdlib for the native-consumer
  fixture; native extension adapters are covered by FFI tests and crate checks.
- Runtime formatting, GC store/root inventories, address classification, test
  registration and root-debt comparisons against main pass. The broad script
  lint run passed 70/76 checks; remaining failures concern missing jq, Windows
  shell fixtures, the workspace-wide formatting command's Windows argument
  length limit, public benchmark freshness, and backslash path keys in the
  absolute unrooted-local inventory. No debt ceiling is raised versus main.
