# Arguments construction benchmark (#10063)

`function-arguments.ts` is the unchanged standalone workload from
[issue #10063](https://github.com/PerryTS/perry/issues/10063). Its SHA-256 is
`f1ae42e13cdf29dec7829aa402c3daf22f093f4a17911dabbc0743c4e105ba73`.
The local `package.json` preserves the original standalone file's CommonJS
context, including sloppy mapped arguments, despite the repository's root
`"type": "module"` setting.

## Reproduce

Use Node v26.5.1 and the same Rust/LLVM toolchain for both Perry builds. In
separate clean worktrees for the baseline and patched revisions, build the CLI
and both runtime archives together:

```sh
cargo build --release --locked -p perry -p perry-runtime-static -p perry-stdlib-static
```

For each build, point `PERRY_RUNTIME_DIR` at that worktree's `target/release` and
compile this same source with `--no-auto-optimize` into separate executables:

```sh
PERRY_RUNTIME_DIR=/absolute/baseline/target/release \
  /absolute/baseline/target/release/perry compile \
  /absolute/patched/benchmarks/issue-10063/function-arguments.ts \
  --no-auto-optimize -o /absolute/results/before
PERRY_RUNTIME_DIR=/absolute/patched/target/release \
  /absolute/patched/target/release/perry compile \
  /absolute/patched/benchmarks/issue-10063/function-arguments.ts \
  --no-auto-optimize -o /absolute/results/after
python benchmarks/issue-10063/measure.py \
  --source benchmarks/issue-10063/function-arguments.ts \
  --before /absolute/results/before --after /absolute/results/after \
  --output /absolute/results/comparison.json --rounds 3
```

On Windows, use `perry.exe`, `.exe` output paths, and PowerShell's
`$env:PERRY_RUNTIME_DIR = 'C:\absolute\worktree\target\release'` syntax.
Ensure LLVM's binaries are on `PATH`. Do not set a different GC backend for one
build or run other builds or benchmarks during the comparison.

The driver serializes every process, alternates the two native builds' order,
enforces a 60-second whole-process timeout, and checks all five checksums. The
workload retains its original warmup (at least 200 ms and five runs), seven
samples, and at least 20 ms per sample. Setup, checksum validation, and process
startup are outside the reported `ms_per_run`; `wall_seconds` includes them.

## Diagnosis

A Windows program-counter sample of the baseline's 10,000-element workload
recorded 1,298 samples. Among the 60 most frequent symbols, shape bookkeeping
accounted for 445 self samples, GC for 252, and descriptor helpers for 85.
Symbols were resolved from the native linker's map; these are self samples,
not inclusive stack costs. The hot path repeatedly appended argument keys,
registered redundant all-true indexed descriptors, and changed semantic shapes.

The fix constructs ordinary arguments objects in bulk. A bounded, GC-traced
cache shares immutable key arrays for arities 0 through 64. Each call still gets
its own values, identity, descriptors, and mapped boxes; key mutation uses the
existing copy-on-write mechanism. Indexed descriptors use their existing
all-true defaults, and the two initial sloppy descriptors share one invalidation
and shape transition.

## Measurements

Measured on 2026-09-11 on Windows x86_64, AMD Ryzen 5 7640HS (12 logical
processors), Node v26.5.1, LLVM 22.1.8, and the Rust nightly 2026-08-20 toolchain.
Both Perry builds report 0.5.1532 and use the same release profile (optimization
level 3, thin LTO, one codegen unit). The baseline is
`603b074ace01464bc66fc07cc8d532f26ccf5a0f`. Timing used the default native
statepoint backend and no concurrent builds or test suites.

Each cell is the median of three processes' reported medians, in milliseconds
per workload run. The million-call baseline produced no result within the
60-second process budget in any round; its speedup cannot be quantified here.

| Calls | Node | Before | After | Speedup |
| ---: | ---: | ---: | ---: | ---: |
| 100 | 0.002337 | 0.7342 | 0.2108 | 3.48x |
| 1,000 | 0.02261 | 7.413 | 2.228 | 3.33x |
| 10,000 | 0.2254 | 75.15 | 21.02 | 3.57x |
| 100,000 | 1.940 | 1,697 | 302.5 | 5.61x |
| 1,000,000 | 18.54 | timeout (3/3) | 3,317 | n/a |

All patched runs and all completed baseline runs matched Node's checksums.
The patched million-call processes took 40.109, 40.984, and 42.046 seconds in
total, including warmup and all seven samples. Their reported medians ranged
from 3,311.6 to 3,507.8 ms per workload run. Perry still has considerable overhead
relative to Node; these results establish the improvement on this Windows host,
not a direct comparison with the issue's original macOS measurements.

See [results-windows.json](results-windows.json) for every observation, timeouts,
toolchain details, and artifact/source hashes.

## Semantic validation

`cargo test --release --locked --lib -p perry-runtime arguments -- --test-threads=1`
passes all 10 selected tests, including the four new construction and moving-GC
regressions. The full runtime library suite passes 3,439 tests, ignores four,
and fails `emergency_full_trace_is_excluded_from_ordinary_pause_stats` on this
Windows host. That unchanged telemetry test expects allocator trimming to be
unsupported, whereas the unchanged mimalloc path reports that it executed; it
also fails in isolation.

With Test262 pinned at `4249661388e5d3f92a85186213da140a6481490f`, both builds
pass 246 of 261 `language/arguments-object` cases. All 15 remaining failure
paths, buckets, and reasons match exactly. Both builds also pass eight of the
nine repository fixtures selected by `run_parity_tests.sh --filter arguments`;
`test_issue_3580_arguments_object_semantics` produces the same existing mismatch.

For those conformance runs, set `PYTHONUTF8=1` so Python writes valid UTF-8
harness files on Windows, and `PERRY_RS4GC=0` for both builds. The default native
statepoint backend currently rejects the Windows exception-handling code used
by most of these tests. The timings above use the default backend, and the new
Rust GC tests independently assert actual evacuation of cached keys, escaping
arguments objects, and indexed heap references.

Runtime formatting, test registration, file-size, GC store, address-class,
root-holder, rekey, and raw-handle audits pass. `pre-tag-check.sh --quick` reports
two remaining checkout/host limitations: workspace formatting exceeds Windows'
command-length limit, and the public benchmark evidence is stale (also observed
on the baseline). No workspace version or release metadata was changed.
