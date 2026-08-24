# Issue #8692 benchmark evidence

Measured 2026-08-24 on an Apple M1 Max running Darwin 25.5.0. The issue
worktree is based on commit `8224d879a`; the baseline arm uses the same branch
build with `PERRY_TYPED_ARRAY_RMW=0`. Node is v26.5.1. All Perry inputs were
compiled with `PERRY_NO_AUTO_OPTIMIZE=1`, the release runtime, `--no-cache`, and
no PGO.

## Reduced reproduction

The input is `repro.js`: 1,000 `Uint32Array` elements, 2,000 iterations, and
2,000,000 total dynamic indexed updates. Both arms and Node returned checksum
`2000`.

Protocol: three warmups followed by 11 alternating enabled/disabled process
pairs. Times below are medians of the elapsed time measured inside the program.
RSS is the median of three `/usr/bin/time -l` process runs. Binary sizes are
exact bytes.

| Build | Median | Paired wins | RSS | Executable |
| --- | ---: | ---: | ---: | ---: |
| `PERRY_TYPED_ARRAY_RMW=0` | 80.315 ms | — | 13,500,416 B | 14,734,768 B |
| guarded direct RMW | 14.760 ms | 11/11 | 13,565,952 B | 14,734,768 B |
| Node v26.5.1 | 2.620 ms | — | — | — |

The guarded lowering is **5.30x faster** than the disabled baseline. RSS rises
by 65,536 bytes (0.49%) and executable-size delta is zero. This result does not
claim Node parity.

The optimized specialized function's `ta.rmw.load`/`ta.rmw.store` blocks contain
`load i32`, `uitofp`, `fadd`, and `store i32`; they contain none of
`js_typed_array_index_get_dynamic`, `js_dynamic_string_or_number_add`, or
`js_typed_array_index_set_dynamic`. The emitted IR retains a full generic
get/add/set block for precondition failure and a set-only block for post-RHS
invalidation. The native-representation artifact records
`TypedArrayRmw.guarded_direct_uint32_add` as `checked_native`, its exact-index and
bounds guard, the GC-visible receiver reload, and a separate explicit dynamic
fallback record. A compiler test also records the rejection reason
`rhs_not_canonical_number`.

## `ecs-benchmark` simple iteration

Source: `ooflorent/ecs-benchmark` at
`7b53a36606118e8b2a450a2ba4919939c86bbd2e`. Each wrapper imports the repository's
unchanged `simple_iter` case and calls `setup(1000)`. Iteration counts were
calibrated per case to keep Perry samples near the upstream harness's roughly
500 ms target. Node, enabled Perry, and disabled Perry all returned the same
case name, iteration count, and `semantic: "completed"` record.

After one process warmup, seven enabled/disabled pairs ran simultaneously so
both arms saw the same shared-host load; child creation order alternated. RSS
was captured on every measured process. The table reports the median elapsed
time of each arm. The upstream README documents typical run-to-run variance of
1–4%, so the increases below are neutral. More decisively, the final Mach-O
`__text` section is byte-identical between enabled and disabled binaries for all
four cases: this optimization is not selected by these workloads.

| Case | Iterations | Disabled | Enabled | Delta | Disabled / enabled RSS | Size delta | `__text` SHA-256 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `wolf-ecs/simple_iter` | 250 | 254.540 ms | 259.059 ms | +1.78% | 202,113,024 / 202,244,096 B | 0 B | `b2987e304a8b402d699d80d145a1a1b01391e83591a36e130a4591ffcd9424f6` |
| `becsy/simple_iter` | 15 | 169.343 ms | 167.251 ms | -1.24% | 46,415,872 / 46,432,256 B | 0 B | `8bdfe53cc561edd2d268b5ec61939ca46bd2140f98303471496b81ab7f7302b1` |
| `javelin-ecs/simple_iter` | 15 | 284.079 ms | 287.036 ms | +1.04% | 93,716,480 / 93,749,248 B | 0 B | `ec44c45d2a942a29877795c85b68ed218b13c7c0f0073892ad90437485b5cc46` |
| `piecs/simple_iter` | 1,500 | 736.326 ms | 751.261 ms | +2.03% | 16,171,008 / 16,203,776 B | 0 B | `cd1d310b8ad30a1f90e6dba95ca82dbe4cd8d344188a3868f9744168a7336b8a` |

Exact enabled/disabled executable sizes were respectively 14,949,648;
16,436,632; 26,191,360; and 14,966,232 bytes for the four rows.
