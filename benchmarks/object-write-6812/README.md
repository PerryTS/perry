# Object-write coverage benchmark (#6812)

This directory preserves the source and measurement corpus for the guarded
object-write generalization tracked by GitHub issue #6812.

`canonical.ts` is intentionally byte-for-byte equivalent to the historical
#6759/#6809 acceptance source quoted in the issue. Do not scale or otherwise
change it; use a separate matrix cell when a longer timing window is needed.

`baseline.md` contains the immutable pre-implementation matrix, classification,
profiles, and raw samples. `results.md` contains the bounded 1–4-field
implementation evidence, final samples, IR/code-size comparison, and validation
results.

Build the compiler and static archives from a clean worktree with an isolated
target directory:

```bash
PERRY_OBJECT_WRITE_TARGET=/absolute/path/to/an/isolated/cargo-target
CARGO_TARGET_DIR="$PERRY_OBJECT_WRITE_TARGET" \
  cargo build --release \
    -p perry -p perry-runtime-static -p perry-stdlib-static
```

Compile with the default auto-optimizing pipeline:

```bash
PERRY_RUNTIME_DIR="$PERRY_OBJECT_WRITE_TARGET/release" \
  "$PERRY_OBJECT_WRITE_TARGET/release/perry" \
  benchmarks/object-write-6812/canonical.ts -o bin_write
```

Do not set `PERRY_NO_AUTO_OPTIMIZE`, override release codegen units, or reuse
archives from another worktree. Alternate Node and Perry runs on the same idle
host and report all raw samples plus the medians.

`matrix.ts` contains the required coverage/rejection cells. Compile it once,
then use `run_matrix.py` to warm each implementation, verify the checksum, and
collect the raw in-program `Date.now()` samples in strict Node/Perry order:

```bash
PERRY_RUNTIME_DIR="$PERRY_OBJECT_WRITE_TARGET/release" \
  "$PERRY_OBJECT_WRITE_TARGET/release/perry" \
  benchmarks/object-write-6812/matrix.ts -o bin_matrix

python3 benchmarks/object-write-6812/run_matrix.py \
  --perry ./bin_matrix --samples 15
```

The runner emits machine-readable JSON first, followed by a median summary and
a collapsible table containing every raw sample and checksum. Redirect or
capture the complete output when collecting evidence; a medians-only excerpt
is not sufficient to reproduce a result.

For a rejected bounded clone, set
`PERRY_TRACE_OBJECT_ARRAY_WRITE_GUARD=1` when running the Perry executable to
print the cold preflight rejection reason. The trace is disabled by default and
is not reached on successful guards.
