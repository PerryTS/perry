# Indexed scans over private numeric arrays (#13661)

## Change and proof boundary

A late HIR pass recognizes private module `const` arrays containing one to eight
numeric literals. An exhaustive LocalId occurrence census must agree with the
accepted reads. Aliases, exports, mutation, captures, unsupported uses, and
unproven indexed reads reject the entire array. Direct eval/deferred dynamic code
also disables the pass. This is construction-and-use evidence, independent of
TypeScript annotations.

An indexed read qualifies only for a literal own index or a `for` counter with
an exact small initial value, `<` bound, one increment writer, and no captures.
A live array-length bound proves reads of that same array only. An out-of-bounds
read must reject even length folding: an inherited getter receives the array as
`this` and can mutate or escape it. The original allocation and TDZ checks remain.

Accepted lengths become constants, and accepted indexed reads become balanced
selections of literal values. There is no runtime side table, cache, array-header
change, or invalidation protocol. Loops are not unrolled by this pass. The separate
`j++` representation-admission issue #13660 is outside this change.

Codegen carries the selected numeric value through constant bindings. A separate
integer-range proof accepts only i32 literal leaves and conditions consisting of
booleans or comparisons of proven primitive numeric values. Conditions still run;
calls, assignments, getters, and possibly coercing comparisons cannot establish
that range. Negative zero, NaN, fractions, and overflowing values do not enter the
new integer-selection path. A coalescing expression can preserve a proven integer
left operand because it cannot evaluate the nullish fallback.

## Reproduction

`benchmarks/bench_indexed_numeric_scan.ts` retains all ten variants from the issue.
`benchmarks/measure_indexed_numeric_scan.py` executes both native binaries and the
Node oracle at 5M and 10M probes, rotating variant/arm order across three rounds.
Every sample checks warmup 42188 and checksums 2109378 / 4218750. The reported
instruction slope is `(median(I_10M) - median(I_5M)) / 5M`; raw process instruction
counts include startup and are retained in the accompanying CSV.

The baseline compiler was built from clean commit
`ee0d9183d4496489dd4532247a64c66b57969230` before editing source. Both compilers use
Rust profile `perry-dev`, LLVM 22.1.8, native LLVM optimization `s`, specialized ABI
on, automatic optimization off, and `--no-cache`. Both link exactly the same
locally built `perry-runtime-static` and `perry-stdlib-static` archives from that
commit, with matching GC-effects sidecars and an explicit `PERRY_RUNTIME_DIR`.
The runtime archives were built with `CARGO_INCREMENTAL=0`. This isolates the
compiler change; these absolute counts should not be compared with the issue's
release-build counts.

Host: Apple M1 Max, 64 GiB RAM, macOS 26.5 (25F71), Node 26.12.1. Other builds were
running on the host, so wall-clock fields are recorded for reproducibility but
are not used to claim speedups.

```sh
LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm@22 \
  cargo build --profile perry-dev -p perry -j4
# Build identical runtime wrappers for both arms and generate matching sidecars.
CARGO_INCREMENTAL=0 LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm@22 \
  cargo build --profile perry-dev -p perry-runtime-static -p perry-stdlib-static
perry-gc-effects --root "$PWD" sidecars target/perry-dev
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" PERRY_NO_AUTO_OPTIMIZE=1 \
  PERRY_KEEP_SYMBOLS=1 PERRY_LL_OPT_LEVEL=s PERRY_SPECIALIZED_ABI=1 \
  target/perry-dev/perry compile benchmarks/bench_indexed_numeric_scan.ts \
  -o /tmp/indexed-candidate --no-cache --trace llvm \
  --strict-eval --strict-dynamic-import --strict-unimplemented
python3 benchmarks/measure_indexed_numeric_scan.py \
  /tmp/indexed-baseline /tmp/indexed-candidate --node /path/to/node-v26.12.1 \
  --output /tmp/indexed-scan.csv
```

Runtime archive SHA-256 values used by both arms:

```text
6f5df9b189ee59e548593919b8f52f5cc3d1f56dd68b925c6d861eb7aa48923e  target/perry-dev/libperry_runtime.a
bff1630fb0c147e0fe5857683a2fbca869bd239e720d34d4efcb8b9293b075e3  target/perry-dev/libperry_stdlib.a
```

## Instruction results

| Variant | Baseline instructions/probe | Candidate | Reduction | Node 26.12.1 |
| --- | ---: | ---: | ---: | ---: |
| method | 1049.54 | 629.91 | 40.0% | 175.44 |
| local | 1001.22 | 494.93 | 50.6% | 174.65 |
| live-bound | 785.10 | 495.15 | 36.9% | 173.24 |
| fixed-bound | 667.34 | 494.39 | 25.9% | 173.30 |
| fixed-typed | 787.70 | 787.07 | 0.1% | 167.13 |
| directions | 410.24 | 409.89 | 0.1% | 153.76 |
| arithmetic | 428.83 | 428.69 | 0.0% | 142.17 |
| field | 983.15 | 474.92 | 51.7% | 179.16 |
| typed-local | 951.28 | 449.21 | 52.8% | 176.26 |
| flat | 1040.02 | 515.24 | 50.5% | 176.50 |

All 180 samples matched the expected warmup and result. The live-bound/fixed-bound
gap falls from 117.76 to 0.76 instructions/probe. Fixed-bound falls by 172.95
instructions/probe; the remaining gap to the direction control is about 84.49.
The fixed-typed and arithmetic controls are effectively unchanged. This is a
microbenchmark instruction reduction, not an end-to-end application speedup claim.

## Emitted code and remaining cost

The pre-optimization LLVM function
`perry_method_repro_ts__AccessBoard__liveBound$idx_u31_28` shrinks from 609 to
566 lines. Its length-helper call, direction-element lookup, dynamic-add helper,
and number-coercion helper disappear; the square addition is `fadd double` and
its local has ordinary double storage instead of a GC root word. The loop bound
is the literal `8`, and the selected direction is a numeric-literal phi tree.

| Static call sites in that LLVM function | Baseline | Candidate |
| --- | ---: | ---: |
| `js_value_length_property_f64` | 1 | 0 |
| `js_dynamic_string_or_number_add` | 1 | 0 |
| `js_number_coerce` | 1 | 0 |
| `perry_array_read_cold_shared` | 3 | 2 |
| `perry_array_index_normalize_i32` | 1 | 1 |
| `js_rel_lt` / `js_rel_ge` | 1 / 1 | 1 / 1 |
| `js_gc_loop_safepoint` | 2 | 1 |

This is not a claim that every numeric/index proof reaches every consumer. The
fixture's generic u31 specialization still has exact index normalization and
boxed comparison fallback sites. Its parameter contract is broader than the
caller's actual `i & 127`; adding a positive direction to an arbitrary u31 value
can exceed i32. The new codegen unit test exercises a genuinely bounded origin
and checks that the complete selected-value/add/const/index chain eliminates
normalization; fractions, NaN, overflow, and dynamic leaves keep it. Further
cross-guard and call-site range propagation is still possible.

The board read retains receiver/layout, forwarding, length and hole checks,
raw/boxed element handling, and a cold path that can resolve inherited access.
Loop control, the 0x88 test, actual board data loads, early-return decisions,
receiver rooting and collection polls also remain. These are a mixture of
necessary work and opportunities for stronger proofs, not a measured waste
percentage.

In optimized ARM64 assembly, the entire
`perry_fn_repro_ts__scan$spec_b_i32_b` (all mode branches) shrinks from 3155 to
2765 machine instructions. Its static length-helper sites go from five to zero,
dynamic-add sites eight to three, number-coercion sites eight to three, and cold
array-read sites eighteen to eight. These are static sites across all variants,
not executed counts or sample shares. Inspect using:

```sh
llvm-objdump --no-show-raw-insn \
  '--disassemble-symbols=_perry_fn_repro_ts__scan$spec_b_i32_b' /tmp/indexed-candidate
```

## Correctness checks

- Transform unit suite: **270 passed**.
- Codegen unit suite: candidate **2630 passed, 14 failed, 1 ignored**; pristine
  baseline **2628 passed, the same 14 failed, 1 ignored**. Both new tests pass.
  A serial candidate rerun has the same failure set. Existing failures include
  IR-shape assertions in prototype, array-read, literal-descriptor, method
  shape, startup descriptor and char-code admission tests; this PR does not
  rewrite unrelated assertions to turn the suite green.
- Thirteen native fixtures are byte-identical to Node 26.12.1 in both arms:
  `test_parity_issue_13661_immutable_numeric_scan`,
  `test_parity_numeric_assignment_contract`,
  `test_parity_indexed_owner_loop_contracts`,
  `test_gap_packed_loop_proto_index_rooting`,
  `test_gap_7853_declared_array_length_runtime_value`,
  `test_gap_region_module_global_arrays`,
  `test_gap_array_fill_receiver_reassign`, `test_gap_small_array_capacity`,
  `test_gap_array_record_numeric_poll_refresh`, `test_gap_repsel_gc_stress`,
  `test_gap_10718_array_rmw_dense`, `test_gap_9201_array_named_properties`, and
  `test_gap_10086_array_destructuring_fast_path`.
- Local chess application `tests/run.ts` (engine and TUI): **875 assertions** in
  Node, clean baseline and candidate, with byte-identical complete output and
  search fingerprint. Includes six perft positions, legal moves/FEN/undo,
  promotion/en-passant/castling/draws, all ten AI levels, search budgets,
  determinism/state restoration, and TUI command/keyboard decoding. This is
  full-application correctness validation; no application timing claim is made.

The application source files `tests/run.ts`, `engine/src/chess.ts`, and
`tui/src/app.ts` match chess commit `f348a206852c982aedef0d252fd7d67659e0ee32`;
unrelated local work in that checkout was left untouched.

### Moving GC and gate limitations

Built the same runtime/static-wrapper source with `PERRY_RUNTIME_INSTRUMENTS=1`
and `CARGO_INCREMENTAL=0`, installed the resulting `_instr` archives beside the
ordinary ones, and regenerated GC-effects sidecars. Ordinary archives were
restored byte-for-byte; performance measurements use those ordinary archives.
`perry-dev` alone does not enable runtime diagnostic instrumentation.

With `PERRY_GC_INSTRUMENTS=1` at compile time, the existing
`test_gap_gc_byte_view_rooted_identity` passed
`scripts/gc_repsel_matrix.sh --arms loop_polls --pressure 8 --jobs 1 --no-build
--profile perry-dev`: **1 PASS, 0 UNVER, 0 FAIL**, byte-identical to Node. The
matrix reports 80 completed cycles, 7199 copied/promoted objects in copying
minors, and a positive relocation counter; its liveness gate passes. The
fixture supplies seed 41, rate 1, allocation interval 0, force evacuation,
evacuation verification, and from-space protection.

The new regression fixture also matches Node under those same seeded,
protected, verified moving-GC settings. Its runtime schedule diagnostic reports
**180 forced collections, 180 copying minors, 7307 moved objects**, and 174 loop
polls. This exercises both folded reads and retained fallback behavior while
objects move.

The unseeded `test_gap_repsel_gc_stress` PR-arm matrix matches Node in all seven
arms, but six arms are **UNVER** because they do not exercise their required
collector path at this revision/configuration. The liveness gate correctly
fails; these cells are not counted as moving-GC passes. The pristine baseline
has the same result with the same instrumented archives.

`git diff --check` passes. The repository-wide file-size gate reports six
unchanged baseline files over 2000 lines (`perry-hir/src/walker/expr_clone.rs`
and five `perry-wpfacts` files: `builtins.rs`, `gen/export.rs`, `gen_rest.rs`,
`model.rs`, `solve.rs`). All changed Rust files remain within the limit.
