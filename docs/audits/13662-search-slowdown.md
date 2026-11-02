# #13662 search slowdown investigation

## Finding

The initial **+5.28% wall-time** result was not a stable estimate of a code
regression. Search was timed while unrelated builds ran. Follow-up runs,
identical-binary controls and scheduler traces demonstrate substantial
preemption and differing P/E-core placement. The old samples cannot be
retroactively decomposed, so this does **not** prove that every part of that
historical difference was noise.

There is also a smaller, identifiable code-generation cost: size-optimizing
three array helpers creates outlined calls on paths that execute during
search. Removing those three annotations recovers most of the extra retired
instructions. This does **not**, by itself, prove a wall-time improvement.

The PR now keeps `array::append::grow`, `read_cold_entry` and
`read_cold_dispatch` speed optimized. It has **195** size annotations in
**119** runtime files. No algorithm, layout representation, side table, cache,
GC rooting, ABI or unwind-policy change is introduced.

## Isolate the measurements

Reuse the independently built reference and initial candidate binaries from
the [original audit](13662-cold-runtime-size.md); do not rebuild between timing
samples. The follow-up uses the same 250,000-node search, same compiler and
expanded feature set. All checksums and search details match.

The old seven paired search rounds range from **−23.83% to +41.90%** wall time.
The new experiment uses six balanced ABBA/BAAB blocks, interspersed with two
placebo blocks whose A and B labels execute the **identical reference binary**.
An adjusted runtime is then tested in four three-arm blocks, each run forward
and reversed (24 total samples). Compare within-block arithmetic means, then
aggregate ratios with a geometric mean. Raw data preserve each sample/order.

| Experiment / arm vs reference | Inner wall Δ | Instructions Δ | Cycles Δ | User CPU Δ |
| --- | ---: | ---: | ---: | ---: |
| Balanced repeat / initial candidate | +2.452% | +0.126% | +0.939% | +0.909% |
| Three-way ablation / initial candidate | -0.452% | +0.137% | +0.202% | +0.241% |
| Three-way ablation / adjusted candidate | +6.046% | +0.031% | +1.036% | +1.526% |

One identical-binary placebo block produces **+3.66% wall time, +1.12% cycles,
and +0.15% instructions** between its nominal arms. The initial candidate's
wall-time direction changes across the two experiments. The adjusted arm's
unfavorable wall/cycle measurements are retained above: do not select only its
better instruction count and call it a proven speedup.

`compare-workload.py` provides balanced runs for JSON-emitting workloads and
records wall/user/system time, retired instructions, cycles, output equality,
binary hashes and compiler-process counts. For an identical-binary placebo,
pass the same executable under two labels. Source copies of this investigation's
initial runners used the recorded mixed placebo and three-arm block orders;
the permanent helper rotates forward/reverse orders. Neither assumes that
zero compiler processes means an idle machine.

## Scheduler evidence

Separate `xctrace` CPU Counters recordings launch each unstripped binary with
`search 250000`. These runs are diagnostics, not additional timed benchmark
samples. Export `thread-state` and select only the target process main thread.
`Running` rows identify their P/E core directly; sum interval durations by
state and core. Raw intervals are conservatively aggregated into 10 ms CSV
buckets, preserving totals and omitting unrelated process information.

| Arm | Running s | P-core s | E-core s | Preempted s | Blocked s |
| --- | ---: | ---: | ---: | ---: | ---: |
| reference | 6.115493 | 6.074402 | 0.041091 | 0.088623 | 0.020470 |
| cold | 6.291509 | 6.129454 | 0.162055 | 0.688728 | 0.021114 |
| no-array | 6.006481 | 6.002590 | 0.003891 | 0.030141 | 0.552672 |

The initial candidate spends **0.600 seconds more preempted** than the reference
in these traces—larger than the original approximately 0.489-second inner-time
gap. It also executes for about **0.162 seconds on E cores**, versus **0.041**
for the reference. This directly establishes scheduling/placement confounds;
it is not a claim that these later traces reproduce the original scheduler
history. The adjusted trace has an early 0.551-second blocked interval and
less running CPU time than the reference, further illustrating why total
elapsed time and execution cost must be separated. Do not attribute that
blocked interval to a particular subsystem without another trace.

Reproduce the recording/export (replace the executable path and trace name):

```sh
xctrace record --template 'CPU Counters' --no-prompt --time-limit 20s \
  --output search.trace --launch -- /path/to/executable.symbols search 250000
xctrace export --input search.trace \
  --xpath '/trace-toc/run[@number="1"]/data/table[@schema="thread-state"]' \
  > thread-state.xml
```

Unstripped copies must be executable. A first adjusted recording failed to
launch because its copied symbol file lacked execute permission; that failed
recording is excluded, and the successful retry is the one reported above.

## Code-generation evidence and targeted change

Four separate four-second `sample` recordings per arm, starting 0.5 seconds
after launch, account for 12,744 reference and 12,602 initial-candidate main
thread samples. Initial-candidate outlined fragments receive **18 samples
(0.143%)**, versus zero in the reference. This is much smaller than the older
blanket-Oz issue profile and is not a removable-overhead estimate.

Generic read dispatch occurs on stacks accounting for about **5.3%** of each
arm's samples, and array growth for about **1%**. Their own self samples are
small; inclusive samples must not be charged entirely to the helper. The
important observation is that these supposedly cold fallback paths do execute
in steady search. `#[cold]` is an optimization hint, not workload evidence.

Disassembly shows all 150 generated engine functions retain their opcode
sequences between the reference and initial candidate. Address operands and
code/data placement can still differ, so this does not rule out layout effects.
The changed array helpers have these static instruction counts:

| Helper | Reference | Initial size variant | Outlined call sites | Adjusted |
| --- | ---: | ---: | ---: | ---: |
| `read_cold_entry` | 83 | 76 | 4 | 83 |
| `read_cold_dispatch` | 300 | 197 | 31 | 300 |
| `array::append::grow` | 135 | 59 | 10 | 135 |

For example, the initial dispatch calls a shared fragment just to prepare two
arguments:

```asm
bl OUTLINED_FUNCTION_740
; Shared fragment:
mov.16b v0, v9
mov.16b v1, v8
ret
```

Inlining those two moves avoids the extra branch-and-link and return. Other
shared fragments load runtime state or restore registers. This is a concrete
size/speed tradeoff on an executed path. It does not explain the entire noisy
wall-time difference.

The ablation removes only the three `optimize(size)` attributes and rebuilds
the exact static wrappers with the same O2/fat-LTO profile. All three resulting
functions match their reference opcode sequences and have **zero outlined
calls**. The three-way comparison reduces the candidate's instruction delta
from **+0.137% to +0.031%**. It does not establish a timing win; the table above
retains the contrary timing measurements.

## Adjusted candidate checks

The engine binary is **6,982,912 bytes**, saving **48,688 bytes** against the
7,031,600-byte reference. The initial candidate was 6,966,512 bytes. Saving
another 16,400 file bytes is not sufficient reason to introduce the avoidable
outlined calls on these active array paths.

Six rebuilt native iterator, exception and GC-related fixtures match Node
26.12.1. These are ordinary parity checks, not a new claim of instrumented live
movement. The earlier 6,365-unit-test run and positive moving-GC tests belong
to the initial candidate; they are not relabeled as runs of the adjusted build.
The public scan is rebuilt against all three expanded-feature runtimes and
measured for nine rotated rounds, with both expected checksums verified:

| Mode | Arm | Bytes | Instructions G | Cycles G | Inner ms | Process s |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| forof | reference | 5,634,688 | 5.388032 | 0.999466 | 320.484 | 0.33 |
| forof | cold | 5,602,272 | 5.387692 | 0.999033 | 319.619 | 0.33 |
| forof | adjusted | 5,602,256 | 5.388149 | 1.002933 | 318.957 | 0.33 |
| indexed | reference | 5,634,688 | 4.255884 | 0.759850 | 240.367 | 0.25 |
| indexed | cold | 5,602,272 | 4.255812 | 0.758425 | 241.460 | 0.25 |
| indexed | adjusted | 5,602,256 | 4.255940 | 0.760650 | 241.730 | 0.25 |

The updated comparison helper passes a real identical-binary scan smoke run;
Python syntax checks and `git diff --check` pass. No new runtime state is added.

The investigation establishes the extra-instruction mechanism and substantial
scheduling confounds. It **does not certify the strict no-regression goal**.
The PR remains draft: a controlled full-application timing confirmation is
still required, and no 4.43 MiB/original-speed result is claimed. The prior full
suite/application checks remain evidence for the initial candidate only.

[Raw samples, scheduler buckets, hashes, disassembly checks and parity results](../../benchmarks/runtime-size/results/2026-11-02-search/)
are retained for review. Local `.trace` recordings and full assembly dumps
remain in the worktree's ignored investigation directory; private application
source and unrelated process information are not published.
