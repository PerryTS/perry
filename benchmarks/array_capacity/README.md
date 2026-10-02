# Small array capacity (#11744)

`cyclic.ts` is the complete unchanged workload from #11743. Its traversal
checks child-to-parent identities, and checkpoints check the captured latest
root. `growing.ts` measures a million independent arrays growing to 4, 5 or
16 elements, and 100 arrays growing to 100,000 elements.

Build each arm from the same compiler revision, changing only
`MIN_ARRAY_CAPACITY` between them. Build the compiler and BOTH static archive
wrappers together; an rlib-only runtime build does not update the link archive:

```sh
LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm@22 \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
cargo build --release --locked -p perry -p perry-runtime-static -p perry-stdlib-static

# Repeat for each arm, with an explicit directory containing its own archives.
mkdir -p /tmp/baseline /tmp/candidate
PERRY_RUNTIME_DIR="$PWD/target/release" target/release/perry compile \
  benchmarks/array_capacity/cyclic.ts --no-auto-optimize --no-cache -o /tmp/baseline/cyclic
PERRY_RUNTIME_DIR="$PWD/target/release" target/release/perry compile \
  benchmarks/array_capacity/growing.ts --no-auto-optimize --no-cache -o /tmp/baseline/growing

python3 benchmarks/array_capacity/measure.py /tmp/baseline /tmp/candidate /tmp/results
```

Use the exact Node version in `.node-version`. The macOS harness applies a
1 GiB runtime heap budget (`PERRY_GC_HEAP_LIMIT=1024`), checks exit status and full stdout against Node,
rotates baseline/candidate order across three sequential rounds, and records
child user + system CPU time and peak RSS from `wait4`. RSS measurements use
the original source with no census or explicit GC.

For live-heap accounting, use a separate diagnostic copy of `cyclic.ts` that
calls `gc()` at checkpoints 7, 55, 103, `released`, 199, and `released-again`.
Set `PERRY_GC_CENSUS` to an output path. Compare `arrays.capacity_total`,
`arrays.length_total`, and the array row in `by_type` at the same checkpoints.
These runs change GC scheduling and allocate census buffers; their RSS must
not replace the uninstrumented measurements.
