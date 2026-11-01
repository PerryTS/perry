# Runtime size versus scan throughput

Public fixture and reproducible measurement tools for [#13662](https://github.com/PerryTS/perry/issues/13662).
These profiles are experiment controls, not recommended release presets.

`scan.ts` runs short numeric-array scans behind class methods. Both modes must
produce `warmup: 42188` and `checksum: 2109378` at five million iterations.
The internal timer excludes construction and warmup. Hardware counters and
process wall time include both. Do not interpret outlined-function sample
share as entirely removable overhead.

## Reproduce

Use native macOS or Linux checkouts of the reference and candidate revisions.
Build a matching compiler and effects analyzer, retaining the repository's
pinned Rust toolchain and `.cargo/config.toml`. On macOS, set
`LLVM_SYS_221_PREFIX` to your LLVM 22 installation if it is not discoverable.

```sh
cargo build --release --locked -p perry -p perry-gc-effects \
  --no-default-features --features perry/dev-cli,perry-gc-effects/analyzer
```

The runtime-only change can use the same compiler for both revisions. Supply
absolute paths; each output directory owns its runtime build and copied
archives. Run from the candidate checkout:

```sh
python3 benchmarks/runtime-size/build.py \
  --source /path/to/reference --compiler /path/to/perry \
  --effects /path/to/perry-gc-effects --profile o2 --output /tmp/reference-o2
python3 benchmarks/runtime-size/build.py \
  --source /path/to/candidate --compiler /path/to/perry \
  --effects /path/to/perry-gc-effects --profile o2 --output /tmp/candidate-o2
python3 benchmarks/runtime-size/build.py \
  --source /path/to/reference --compiler /path/to/perry \
  --effects /path/to/perry-gc-effects --profile oz --output /tmp/reference-oz
```

Both profiles use fat LTO, the system allocator, and identical minimal
features. App optimization stays at O2 with specialized ABI enabled. The
builder retains unstripped executables as `scan.symbols`, strips the measured
`scan`, and records commit, source diff, profile, features, toolchain and archive
hashes. It preserves exception/unwind configuration and generates GC-effects
metadata from the exact archives used for linking. The two forwarding-wrapper
rule adaptations and the strictly diagnosed absent teardown-cut adaptation
are the fat-LTO compatibility workarounds documented in the issue, not size
optimizations. Unexpected analysis failures stop the build.

After **all builds and profiling have finished**, measure serially on macOS:

```sh
python3 benchmarks/runtime-size/measure.py --rounds 9 \
  --binary reference=/tmp/reference-o2/scan \
  --binary candidate=/tmp/candidate-o2/scan \
  --binary blanket-oz=/tmp/reference-oz/scan --output /tmp/scan-results.json
```

The collector rotates binary order and fails on incorrect results, invalid
timings, or missing/zero hardware counters. It records bytes and hashes along
with every measurement. On Linux, collect the same fixture with
`perf stat -e instructions,cycles` and an external wall timer; the macOS
counter parser deliberately does not silently substitute wall time there.

For programs needing other runtime features, pass the **same** `--features`
list to both builds. Then compile each program against its corresponding
`runtime` directory with `PERRY_RUNTIME_DIR` and `PERRY_NO_AUTO_OPTIMIZE=1`.
A minimal runtime cannot validate features that were compiled out. Check the
regular performance suite, startup, moving GC, exceptions, and representative
applications (including terminal/signal handling), not only this scan.

Platform coverage and measured limitations belong in the accompanying audit;
a faster scan alone does not establish a generally regression-free size preset.

Measured controls, validation and limitations: [audit](../../docs/audits/13662-cold-runtime-size.md).
