# Repeated RegExp construction (#10179)

Run all commands on a build host, with the compiler and runtime/stdlib archives
from the same build. In the issue's Mac lane, wrap every command below in
`GIT_DIR=/tmp/perry-regexp-lane.git ./remote.sh '<command>'`.

`prepare.py` resolves the actual OpenCode v1.18.30 packages under
`$OPENCODE_SRC/node_modules/.bun`: emoji-regex 10.6.0, string-width 7.2.0,
strip-ansi 7.1.2, and get-east-asian-width 1.6.0. Nothing substitutes a shortened
pattern. The current compiler reports 10 modules for this probe.

```sh
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
export RUSTFLAGS="-C force-unwind-tables=yes -C force-frame-pointers=yes"
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
export PERRY_RUNTIME_DIR="$CARGO_TARGET_DIR/release"
python3 benchmarks/regexp-construction/prepare.py /tmp/regexp-probe.ts
"$CARGO_TARGET_DIR/release/perry" compile /tmp/regexp-probe.ts \
  -o /tmp/regexp-probe --no-auto-optimize --debug-symbols
/tmp/regexp-probe all 1
/root/claude-opencode/bun/bin/bun /tmp/regexp-probe.ts all 1
PERRY_REGEX_DIAG=1 /tmp/regexp-probe all 1
perf record -q -F 499 -g --call-graph dwarf -o /tmp/regexp.perf \
  /tmp/regexp-probe construct 100
perf report --stdio --no-children -g none -i /tmp/regexp.perf \
  --percent-limit 1 -F overhead,symbol
```

The first positional argument selects an operation (`construct`, `test-ascii`,
`test-emoji`, `stripAnsi`, `stripAnsi-match`, `ignorable`, `segment`,
`eastAsianWidth`, `stringWidth`, or `all`). The second multiplies iterations.
The emoji test intentionally preserves global `lastIndex` between calls, as
in the issue probe: successful and unsuccessful searches alternate. Compare
identical iteration counts and include the printed checksum. Timing is wall
microseconds per iteration and includes the first operation; larger scales
amortize initial compilation/validation and Bun's warmup.

`measure.py` checks checksums and retains samples, medians, and execution
orders. Native builds run from one fixed pathname/inode; copying happens
outside the timing window. Rotation plus reversal covers all six orders of
before/after/Bun. Use a multiple of six rounds for balanced positions, and
repeat `--case MODE:SCALE` to select individual cases.

```sh
taskset -c 15 python3 benchmarks/regexp-construction/measure.py \
  --before /tmp/regexp-probe-before --after /tmp/regexp-probe \
  --source /tmp/regexp-probe.ts --bun /root/claude-opencode/bun/bin/bun \
  --output /tmp/regexp-probe-results.json --runs 12
```

For existing benchmark regressions, `compare.py` compiles all 12 existing
`benchmarks/app-patterns/kernels/*.ts` with each compiler/archive bundle,
checks outputs against the repository's pinned Node oracle, and alternates
before/after execution order. Each build is copied to the same executable path
outside the timed window. It reports child user CPU as well as wall time;
use identical `--before`/`--after` bundles for an A/A noise control. Both
harnesses also accept `--paired-controls`: two identical-copy labels per build
run in each balanced four-run block (the probe omits Bun in this mode).

```sh
export PATH=/tmp/regexp-oracle/node-v26.5.1-linux-x64/bin:$PATH
python3 benchmarks/regexp-construction/compare.py \
  --before /tmp/regexp-baseline-libs --after "$CARGO_TARGET_DIR/release" \
  --output /tmp/regexp-app-comparison --runs 12
```

See [current-results.md](current-results.md) for the latest integration comparison,
[final-results.md](final-results.md) for the prior Perex 0.1.4 measurements,
[results.md](results.md) for the original attribution, and
[merged-results.md](merged-results.md) for the earlier integration measurements.
