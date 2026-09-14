# Catch-frame setup

Grouping the per-depth exception state and inlining its savepoint providers
reduces non-throwing setup by 121 instructions per runtime catch in this probe.
The existing C trampoline still owns `setjmp`. Generated user `try`/`catch`
calls `js_eh_try_push` in `perry-codegen/src/stmt/try_stmt.rs` and shares the
same snapshot capture, although its exception transport uses the system unwinder.

## Design and correctness

`ExceptionState` keeps fixed jump-buffer and handler-kind slabs alongside one
slab of per-depth `CatchSavepoint` records. Grouping the captured fields removes
their separate savepoint arrays. Keeping jump buffers separately aligned avoids
the submitted all-in-one record's 15 KiB of padding per thread on the default
64-bit build. The slabs never grow, so jump-buffer addresses remain stable.
The 1,024-handler limit and try-depth accounting are unchanged.

Snapshot slots use `MaybeUninit`: a push writes every field before incrementing
`try_depth`, and restoration reads only a published handler's initialized slot.
Inactive snapshots are neither read nor scanned. This also avoids eagerly
writing the nonzero shadow-stack sentinel throughout unused snapshot pages.
The record is `Copy`, so no owned resources need destruction in inactive slots.

`exception/savepoints.rs` declares each snapshot field together with its
capture provider, restore provider and required real-throw witness. The macro
generates storage, capture, ordered restoration and the test. This removes
the independently maintained lists, including the incomplete legacy test
replay. The small capture providers inline so the optimizer can reuse hot
TLS accesses; live subsystem state stays with its current owner. This is an
incremental grouping change, without a journal or additional live TLS mirrors.

Every existing restore remains, in its original order. Async-context cleanup
still runs first, using the unchanged handler depth. No callers lose their
catch frames. A future frame-elision optimization needs separate evidence
that its protected operation cannot throw.

The generated tests establish nonempty state outside a trap, change it again
inside two nested traps, and throw through `js_throw` and the C trampoline.
They require both catches to recover exactly their enclosing state, and also
verify that normal completion does not replay cleanup. The shadow witness
changes both shadow frames and expression temp roots; the dyn-eval witness
changes both interpreter roots and call depth.

`fault_restore.py` temporarily omits one registered restore at a time in the
production path. All 13 omissions fail their corresponding test at the
enclosing-state assertion; results are in `evidence/fault-results.json`.
Each mutant runs in a separate process. The expected assertion aborts across
the C callback boundary, so its exit is SIGABRT. An unrelated crash or an empty
test filter does not count as detection. The script restores and rebuilds the
original source in `finally`, then requires the exception suite to pass.

## Instruction measurements, 2026-09-14

The historical numbers below describe the submitted all-in-one record layout.
The landing train remeasures the adjusted storage against its merged baseline
with the same package selection and records CPU/instruction and RSS results
separately; these historical numbers are not measurements of that adjustment.

Baseline: main `eb13fa188d` (0.5.1564), which already includes the #10215
large-array corruption fix. Host: perrymaster, AMD Ryzen 7 7700X, Linux
6.17.0-23-generic, perf 6.17.13, Rust nightly 2026-08-20
(`1.100.0-nightly f7d782a3b`), LLVM 22. Node oracle: 26.5.1.

Before changing runtime code, the promise/async workload pushed 100,007 frames.
Its initial instruction profile attributed 96 of 4,221 samples (2.27%) to
capture. A later independent baseline sample attributed 29 of 1,098 (2.64%).
The JSON allocation workload pushed only six frames, making it a useful
negative control. The hoisted regex test pushed 3,500,011 frames. Uprobe counts
were collected separately, with the probe removed before performance runs.

| Workload | Baseline instructions | Candidate instructions | Change |
|---|---:|---:|---:|
| Promise/async loop, 100k | 1,098,667,245 | 1,077,465,852 | -1.93% |
| JSON parse/stringify loop, 100k | 3,182,841,469 | 3,171,165,684 | -0.37% |
| Hoisted regex `test`, 1M | 13,816,754,889 | 13,266,297,479 | -3.98% |
| Regex `exec`, 1M | 31,217,404,185 | 30,183,325,430 | -3.31% |
| Runtime catch microprobe, 1M | 472,151,423 | 351,152,420 | -25.63% |
| Generated-handler setup microprobe, 1M | 366,173,508 | 244,175,238 | -33.32% |
| Plain microprobe control, 1M | 27,068,237 | 27,068,989 | +0.00% |

Subtracting each build's plain loop gives **445.08 → 324.08 instructions per
runtime catch (-27.19%)**, and **339.11 → 217.11 per generated-handler setup
(-35.98%)**. These include frame entry and exit; they do not measure throwing.
The Rust example has its own Cargo link graph, so its absolute per-frame cost
should not be substituted into compiled-program profiles.

All four TypeScript outputs match Node. The regex probes are copied from
`/root/claude-regex/repro/tonly/{hoist,exec1}.ts`. No cc application claim is
made. The small JSON delta cannot be attributed to its six catch frames:
provider inlining can also change other call sites and compiler output.

`evidence/instructions.json` records totals, sampled attribution, frame counts
and archive/compiler hashes. Raw stat outputs and gzip-compressed folded
stacks are alongside it. Symbol attribution is approximate under LTO and
sampling. The verdict uses instruction totals. All retained A/B folded stacks
use `perf script --no-inline` consistently. Raw perf data and build logs are
retained under `/root/js-throw-evidence` on perrymaster.

Integration exposed a Thin LTO identity hazard: separately linked archives
could give `global_this_builtin_noop_thunk` different addresses, breaking
builtin constructor recognition. The final patch gives this address sentinel
one external symbol and a distinct non-inlined body. The unguarded optimized
build fails the EventTarget subclass fixture; changing only its constructor
pointer to the comparison target in GDB makes that fixture pass. The guarded
build passes that fixture and four related constructor fixtures. The final
measurements above and the full parity sweep both use a fresh, frozen build
including this guard. See `evidence/constructor-identity.md` for the witness.

## Reproduction

Use isolated baseline and candidate worktrees, each with its own target.
Keep the package selection and build settings identical on both sides:

```sh
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
export CARGO_BUILD_JOBS=4 CARGO_INCREMENTAL=0
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
export CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=none
cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
cargo build --release -p perry-runtime --example catch_frames
export PERRY_RUNTIME_DIR="$PWD/target/release"
python3 benchmarks/catch_frames/measure.py \
  --perry "$PWD/target/release/perry" \
  --micro "$PWD/target/release/examples/catch_frames" --output /tmp/catch-measure
python3 benchmarks/catch_frames/count_frames.py \
  /tmp/catch-measure/promises /tmp/catch-measure/promises.frames
```

Put pinned Node 26.5.1 on PATH. Do not override the repository's unwind-table
and x86 frame-pointer flags. Both measurements use thin LTO and the same
16-codegen-unit override; these are not claims about a separate dist build.
Copy the benchmark sources and example into the baseline worktree without
the runtime patch. Freeze the three compiler/archive outputs before building
any extra package set for other validation.

The measurement script uses `perf stat -r 3 -e instructions:u` and a separate
`perf record -e instructions:u --call-graph dwarf` run. It retains oracle
outputs and folded stacks. Run fault injection only in a disposable worktree,
with no other builds or source copying in progress:

```sh
python3 benchmarks/catch_frames/fault_restore.py --output /tmp/catch-faults
```

## Validation

The local replay and its baseline comparisons are recorded in
`evidence/validation.md`. GC ratchet measurements use seven repetitions of
each of 14 probes. The candidate passes against a fresh Linux baseline from
the same main commit, with all 126 heap/collector medians identical. This
does not replace the repository's pinned artifact with a new baseline or
claim equivalence to its different-platform capture. GC wall/RSS observations
are retained by the harness but are not performance evidence for this change.
