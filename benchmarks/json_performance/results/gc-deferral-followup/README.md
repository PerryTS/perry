# JSON GC deferral follow-up

**The retained runtime remains `0327b9460a749592deb354d72ebad29bf4ae4bba`.**
Two further stringify experiments were tested and discarded. This checkpoint
adds evidence and a compiled regression fixture; it does not change runtime
behavior or broaden GC deferral. The [previous results](../gc-deferral/README.md)
and their unresolved CPU/RSS/semantic requirements still apply.

## What the investigation established

Small-record stringify slows even when its timed loop performs **zero automatic
collections**. The diagnostic traces for 100,000 calls contain only the manual
collections before and after the loop. At one million calls, both binaries run
the same eight automatic cycles with similar summed pauses. Extra collections
do not explain this regression. The stringify entry has the same 324 native
instructions in both binaries, with relocated addresses; that comparison does
not prove every callee is identical.

The 20 MiB round-trip result depends strongly on where the workload ends in the
collection cycle. A repeat with the unchanged deferral checkpoint, three fresh
process pairs and explicit final cleanup, gives these total-CPU changes against
the construction parent:

| Calls | Total CPU change |
|---:|---:|
| 7 | −0.29% |
| 8 | +4.61% |
| 9 | −4.11% |
| 16 | −2.69% |
| 24 | −7.80% |

The eight-call regression remains real for that workload; it is not a universal
4.5% cost per round trip. Measuring only API or loop time can hide the cleanup
tradeoff. These additional counts supplement the original result rather than
replace it. In the same rerun, wide-object parse with discarded/latest-only
outputs still saves **43.27% / 43.86% total CPU**, including cleanup.
Small-record stringify remains **2.40–3.18% slower** across the three lifetimes.
See [the lifetime comparisons](discarded-stack-plan/lifetimes/table.md) and
[the separate GC diagnostics](diagnosis/summary.json).

## Discarded experiments

The first experiment added direct root-record emission to the general walker,
avoiding temporary shape-prefix templates. Correctness checks passed, but the
benchmarked small records already use `stringify_record_output` through the
full entry, before the general walker. The proposed path therefore did not
address the target. Its patch and raw measurements are preserved under
`discarded-root-emitter`. The remote run finished and passed its entry/end host
gate, but the local observer failed while saving data during a transient
disk-full condition. Its incomplete observation record disqualifies it for
performance acceptance; it has not been retried because the source was rejected.

The second experiment used `MaybeUninit` for the direct-output stack plans,
writing only their used entries. The plan still held no managed pointers, and
the final input/output allocation and GC policy were unchanged. Native assembly
confirms the initial stores disappeared. Five repeated three-arm comparisons
showed modest gains, but larger regressions elsewhere:

| Workload | CPU change vs deferral checkpoint |
|---|---:|
| Tiny-object stringify | −0.89% |
| 1 KiB-object stringify | −0.45% |
| 1 MiB record-object stringify | −0.71% |
| Small-record stringify | +0.17%, overlapping ranges |
| Numeric-array stringify | +2.33% |
| Heterogeneous parse | +2.76% |
| 16 KiB / 1 MiB / 8 MiB array parse | +2.62% / +2.61% / +2.37% |
| Wide-object parse | +0.82% |

All listed ranges except small-record stringify were separated. The lifetime
probe recovers 1.78–2.98% of small-record stringify CPU, but the general matrix
regressions make the trade unacceptable. Some retained-RSS rows increase too;
all data is preserved in [38 CPU comparisons](discarded-stack-plan/measurements/table.md)
and [36 retained-memory comparisons](discarded-stack-plan/measurements/memory-summary.json).

On the heterogeneous parse, array parse and numeric stringify regressions,
whole-process retired instructions differ by less than 0.005% while cycles
increase about 2.2–2.6%. Parser source and GC policy did not change in this
experiment. This suggests sensitivity to generated-code placement or other
execution effects; the specific cache/branch mechanism has not been established.
Instruction reductions alone do not establish a CPU improvement.

## Validation and provenance

- The retained runtime passes the new `test_gap_json_root_record.ts` against
  Node 26.5.1 in nine configurations. Four moving configurations force collection
  at every eligible poll, with 70 scheduled collections, 75 copying minors and
  64 loop polls in each run. They cover direct output, fallback, getters,
  `toJSON`, holes, omitted values, numeric key ordering and retained children.
- The discarded stack-plan build passes 3,278 runtime tests (four ignored),
  40 core compiled Node comparisons, the existing extended/forced-tape/control
  checks, and both nine-configuration fixtures. Its 52 moving runs include
  two comparisons to the reference with the documented inherited Node mismatch.
- Its main measurement passes 190 output checks, 570 timing trials and 324
  retained-memory trials, with 33 clean external observations. Entry/end load
  is 1.852 / 2.205, within the fixed 2.5 limit. Its supplemental run has 108
  trials across 12 cases and 27 clean observations, with load 1.985 / 2.378.
- The standalone diagnosis has 54 runs: 18 traced and 36 uninstrumented.
  Traced runs are excluded from its CPU comparison. It is a mechanism probe
  with two uninstrumented pairs per case, not a replacement acceptance matrix.
- Both experimental builds use the pinned application object and runtime/stdlib
  build settings from the prior checkpoint. Source stamps, binary hashes,
  patches and raw results are retained. The private release build directory
  currently contains the **discarded** stack-plan build; the retained runtime
  is the separately pinned `defer2-runtime`, not that build directory.
  The archived patches use zero context; apply them with `git apply --unidiff-zero`
  to the retained source in a separate checkout to reconstruct either variant.
- Node-version and root-holder checks pass. The existing file-size and address
  classification failures remain on files unchanged from the retained source.
  Earlier semantic gaps and the older 147-case matrix are not cleared here.

Run `python3 benchmarks/json_performance/results/gc-deferral-followup/verify.py`
to verify the retained source and recorded counts/correctness/qualification.
Nothing from this follow-up has been merged or published.
