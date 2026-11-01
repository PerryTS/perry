# Runtime size without blanket Oz

The public scan from [#13662](https://github.com/PerryTS/perry/issues/13662)
exercises short numeric-array scans behind class methods. `startup.ts` forces
realm initialization, including the builtin installers, rather than measuring
only a console call that can omit them.

The proposed policy keeps the runtime's speed profile and applies
`#[optimize(size)]` only to builtin installation. It does not change the
installed methods, array/iterator operations, allocation, GC, or exception
transport. `oz.toml` is a diagnostic negative control, **not a recommended
shipping profile**. `speed.toml` reproduces the issue's O2/fat-LTO control;
neither file changes Perry's normal release settings.

[Measured macOS ARM64 results](RESULTS.md) include all 400 observations,
archive fingerprints, startup, the regular suite and complete-application
controls. The measured saving is about 16 KB, with baseline steady-state speed;
the issue's 4.43 MiB/original-speed combination remains unachieved.

## Reproduce

Use separate baseline and candidate checkouts, the repository's pinned Rust
toolchain, and a compiler built from the same revision as the runtime. Keep
the app compiler identical across arms when comparing a runtime-only change.
From each checkout, build the staticlib wrappers into isolated directories:

```sh
CARGO_TARGET_DIR="$PWD/target/size-speed" cargo \
  --config benchmarks/runtime_size_perf/speed.toml \
  build --profile issue-size --locked -j 3 \
  -p perry-runtime-static -p perry-stdlib-static --no-default-features \
  --features perry-runtime/keepalive-anchors,perry-stdlib/async-bridge
```

For the blanket-Oz control substitute `oz.toml` and a different target
directory. Preserve `.cargo/config.toml`, panic strategy and unwind tables.
Generate GC-effects sidecars over each arm's **final archives** before
compiling. The issue documents the two forwarding-wrapper rules and the
precisely diagnosed unmatched teardown cut needed by the analyzer for fat
LTO; do not suppress unknown calls or reuse another archive's sidecars.

```sh
PERRY_RUNTIME_DIR=/absolute/path/to/runtime \
PERRY_KEEP_SYMBOLS=1 PERRY_NO_CACHE=1 \
PERRY_LL_OPT_LEVEL=2 PERRY_SPECIALIZED_ABI=1 \
  /absolute/path/to/perry compile benchmarks/runtime_size_perf/scan.ts \
  -o /absolute/path/to/scan-symbols \
  --strict-eval --strict-dynamic-import --strict-unimplemented
cp /absolute/path/to/scan-symbols /absolute/path/to/scan
strip /absolute/path/to/scan
# macOS: re-sign after stripping.
codesign --force --sign - /absolute/path/to/scan
```

Finish **all** builds before timing. Use the same feature set, allocator,
compiler/app flags, source metadata policy and stripping policy for every arm.
For full applications that require additional runtime features, rebuild both
arms with those same features instead of mixing archives from different
Cargo graphs.

```sh
python3 benchmarks/runtime_size_perf/measure.py \
  --arm speed=/absolute/path/to/baseline/scan \
  --arm cold=/absolute/path/to/candidate/scan \
  --arm oz=/absolute/path/to/oz/scan \
  --rounds 7 --output results.json
```

The runner rotates arm order and executes serially. It checks every result
against an independent 128-origin period, including the 100,000-iteration
warmup (42,188 hits) and the default 5,000,000 probes (2,109,378 hits). Missing
hardware counters and incorrect output fail the run. macOS counters come
from `/usr/bin/time -l`; Linux requires permission for `perf stat` hardware
counters. Other platforms are not covered by this runner.

The JSON records signed/stripped file bytes as supplied, post-warmup scan
time, whole-process wall time (including the counter tool's launch), retired
instructions, cycles, and the raw output for every sample. Preserve symbol
binaries separately for `__text` size and disassembly inspection. Report
startup and the regular performance suite alongside the scan; a smaller
file alone is not a passing result.
