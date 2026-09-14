# Native property Get probes

These probes accompany the 2026-09-14 native-side property Get brief. They
separate positive own/prototype reads from non-regex iterator, promise, and
JSON work. Each program checks its result; the runner also compares stdout
with the repository's pinned Node version and rejects an empty result.

Build the compiler and both static wrappers from the same checkout and
package set. Use an isolated target directory/worktree for each concurrent
build. For the Linux instruction comparison:

```sh
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=none \
cargo build --release --locked -j4 \
  -p perry -p perry-runtime-static -p perry-stdlib-static
python3 benchmarks/native_property_get/measure.py \
  --runtime-dir target/release --out /absolute/path/to/new-results \
  --node /path/to/node-v26.5.1/bin/node
```

The output directory must be new. The runner records the source commit,
compiler/archive/probe SHA-256 hashes, three `instructions:u` counts,
instruction-sampled DWARF call stacks, period-weighted folded stacks, and
inclusive Get attribution. Nested Get frames count once in the inclusive
share. Shares are sampling estimates; the stat counts are the performance
comparison. `--debug-symbols` is required when compiling the probes, and
`--no-auto-optimize` pins the runtime archives. LLVM's addr2line is selected
locally when available so host binutils does not choke on runtime DWARF.

`--smoke` compiles every probe, runs 100 iterations, and compares with Node
without requiring Linux perf. A zero Get attribution is rejected: it can
indicate stripped symbols or a probe that did not exercise the intended path.

The iterator reuses its result object. It avoids large retained arrays and
therefore does not depend on resolving the concurrent #10215 investigation.
The thenable and toJSON probes use positive lookups because main already
optimizes many negative probes for those names.

Measured results, scope, and validation are in [REPORT.md](REPORT.md). To replay
the fault injections with no other builds reading this worktree:

```sh
python3 benchmarks/native_property_get/faults.py --out /absolute/path/to/new-fault-results
```

The three mutants must fail their named test assertions, and restored sources
must pass the eight focused tests. The GC custody inventory classifies the
forced-slow boolean as test-only; production binaries contain no switch.
