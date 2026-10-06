# #12023 RSS follow-up

The [cycles follow-up](statepoints-only-12023-cycles.md) supersedes the short-witness RSS figures here: the previous timing scope included the profiler. It also records the pinned-main rebase, guarded cycle attempts, fresh performance failures, and new correctness receipts.

The ~1.7 MiB commander increase is executable file-backed residency, with a
substantial file-cache/layout contribution. It survives THP off; it is not a
growing GC heap or a megabyte-scale stack change. Equal warming of both immutable
ELF files brings all five workloads' **separate-arm RSS medians** to or below
base in 15 interleaved pairs per THP mode. Small commander/Zod/hello differences
overlap noise; these measurements do not prove a strict nonpositive RSS delta.

No compiler, runtime, allocator, linker, or rooting change was accepted in this
follow-up. The fix is to the measurement protocol: record binary identities,
warm both complete files equally, alternate arms and THP modes, check the pinned
Node oracle, and retain every run with GC counters. The generic Linux harness is
`scripts/measure_linux_rss_ab.py`. It takes a manifest of already coherently built
arms and never changes shared-host cache or kernel settings.

Strict owner acceptance remains **open for CPU**. The reported CPU medians are
not all at or below base; this report does not substitute lower instructions for
that requirement.

## Identities and method

Base: `d3098e903ff9e68cec2a900b9baf3143445d195c`.
Starting head: `6baaa58298bc0b0551d427b7c6d35d969566a4dc`;
its executable code equals the previously tested `00b4e15ee`.
Normal release profile, LLVM 22.1.8, separately pinned compiler/runtime/workspace
arms. Measurements ran on qb6, CPU 31, with `perf stat -e instructions:u`,
`/usr/bin/time`, exact saved Node 26.5.1 output, and `PERRY_GC_DIAG=1`.
THP on/off is process-local `MIMALLOC_ALLOW_THP=1/0`.
There are 15 runs per arm, workload and mode (300 executions).
Both complete ELF files were read before the batch. The committed harness also
repeats this warming before each pair. Raw run metrics, SHA-256 identities, ranges,
all collection counts, and paired RSS differences are in
`/root/claude-lanes/sp-work` (raw receipts retained off-branch).
Full logs/scripts remain in `/root/claude-lanes/sp-work`.

## Attribution before the cache control

15-pair commander RSS medians were 51,340 -> 53,300 KiB with THP on and
25,032 -> 26,804 KiB off. Zod was 56,816 -> 56,948 on and
27,816 -> 27,964 off. Default-THP rows independently reproduced the gap.

Peak sampling of `smaps_rollup`, `smaps`, `status`, and pagemap attributes
commander's extra 1.4–2.0 MiB to executable file-backed mappings.
Anonymous RSS is approximately 38,408 KiB on / 12,280 KiB off in both arms;
AnonHugePages is 36,864 KiB on / zero off in both. Commander has 33 minors,
zero fulls, 575,240 copied bytes and 463,824 promoted bytes in both arms.
Zod's promoted-byte difference is only 1,952 bytes.

VmStk is 132 KiB in both, with 44–52 KiB resident stack. Stack-pointer samples
(7,871 base / 7,849 head) span 28,816 -> 32,128 bytes, an observed difference of
3,312 bytes rather than a 1.7 MiB stack increase. This sampling is not a proof
of the absolute maximum call depth. Physical stack residency independently
bounds its contribution. Hot recursive commander frames:

| Function | Stack subtraction B base -> head | Saved registers B |
|---|---:|---:|
| _findCommand | 40 -> 40 | 48 |
| _parseCommand | 872 -> 920 | 48 |
| _processArguments | 72 -> 72 | 48 |
| parseOptions | 568 -> 504 | 48 |

Commander text size shrinks 19,189,106 -> 19,157,810 bytes, while present text
pages grow 9,542,784 -> 10,824,832 bytes in the sampled executions.
RO data has the same 3,334,700-byte extent but resident bytes grow
741,376 -> 987,136. Some extra pages cover cold telemetry/dyn-eval/sqlite/zstd
code. Function addresses shift about 31 KiB; the observed host fault-around
window is 65,536 bytes. The Linux implementation maps ready adjacent cached
pages ([primary source](https://code.googlesource.com/linux/torvalds/linux/+/e387dc122fc7c70c2a5df2567f4e2d1114f5a5da/mm/memory.c)).
Cache/layout interaction is an inference supported by the following controls,
not an assertion that every extra page was individually fault-traced.

A byte-identical fresh base copy adds approximately 0.9–1.2 MiB RSS in 15
interleaved on/off runs. Comparing fresh copies gives head -176 KiB on / +20 KiB
off. Reading/disassembling the original base changes its residency toward that
of the head. No global page-cache, THP or kernel setting was changed.

## Equal-cache five-workload result

| Workload | THP | Instructions % | CPU s base → head | RSS KiB base → head | Fulls / minors base → head |
|---|---|---:|---:|---:|---|
| tscwork | on | -0.117% | 3.46 → 3.81 | 266,452 → 259,036 | 1/4 → 1/4 |
| tscwork | off | -0.094% | 3.80 → 4.13 | 224,444 → 220,872 | 1/4 → 1/4 |
| zodwork | on | -0.082% | 0.10 → 0.11 | 57,168 → 56,820 | 0/4 → 0/4 |
| zodwork | off | -0.101% | 0.13 → 0.13 | 28,160 → 27,908 | 0/4 → 0/4 |
| qs | on | -0.606% | 3.50 → 3.56 | 59,600 → 57,560 | 0/119 → 0/131 |
| qs | off | -0.614% | 3.39 → 3.27 | 30,876 → 30,648 | 0/119 → 0/131 |
| commander | on | -0.279% | 0.90 → 0.93 | 52,684 → 52,660 | 0/33 → 0/33 |
| commander | off | -0.279% | 1.22 → 1.11 | 26,584 → 26,520 | 0/33 → 0/33 |
| hello | on | -0.051% | 0.01 → 0.01 | 17,960 → 17,944 | 0/0 → 0/0 |
| hello | off | -0.044% | 0.00 → 0.01 | 17,920 → 17,684 | 0/0 → 0/0 |

All table figures are medians. Collection counts shown are identical in every
one of the 15 runs in each arm/mode. TS fulls are **one in all 60 executions**.
No TS instruction regression was observed; the known padding-sweep TABLE was
read earlier, and no new padding sweep is claimed. qs retains its previously
observed different minor count.

| Workload | Executable bytes base -> head |
|---|---:|
| tscwork | 202,438,944 -> 192,134,216 |
| zodwork | 24,946,512 -> 24,758,560 |
| qs | 27,694,832 -> 27,558,752 |
| commander | 34,612,192 -> 34,568,688 |
| hello | 19,026,240 -> 19,020,824 |

Separate-arm medians and the median paired difference need not have the same
sign. For commander on, the former is -24 KiB and the latter is +28 KiB.
The 95% paired bootstrap intervals below are descriptive (10,000 resamples,
fixed seed), not a certified upper bound or an acceptance gate:

| Workload | Paired RSS median KiB on / off | Bootstrap 95% KiB on / off |
|---|---:|---|
| tscwork | -7476 / -3564 | [-8828, -6376] / [-3872, -3180] |
| zodwork | -288 / -204 | [-696, 0] / [-452, 64] |
| qs | -2072 / -152 | [-2236, -1848] / [-372, -28] |
| commander | +28 / -20 | [-160, 128] / [-180, 8] |
| hello | -64 / -64 | [-300, 324] / [-376, 12] |

A further ten paired TS runs collected instructions, cycles, task-clock, cache
and branch misses. Median cycles were 21,966,009,701.5 -> 21,991,997,911.5;
task-clock 6,017.195 -> 6,116.455 ms; CPI 0.7292 -> 0.7309.
Effective user-cycle/task-clock rate was 3.618 -> 3.553 GHz. Individual paired
CPU differences ranged -11.4% to +63.4%. Other-lane rustc was observed on pinned
CPU 31; neither those processes nor shared-host frequency settings were changed.
These data demonstrate interference but cannot establish that the underlying
CPU change is nonpositive. A quiet-core/time-window comparison remains needed.

## Rejected prototypes and correctness

GNU sorting/section ordering/64 KiB alignment, partial text grouping, mold,
MADV_RANDOM, and several LLD layouts were measured without changing primary
source. None met all requirements. LLD -O2/separate-code plus a coherent
-fno-plt C rebuild improved commander/Zod/qs RSS and instructions, but increased
hello RSS and **failed Request signal semantics** in all four GC knob sets.
The failure remains with proper Cargo/LLVM PATH, -O1, --no-relax,
-z nostart-stop-gc, and preferring the stdlib-bundled runtime object.
The linker failure's root cause is unresolved. That candidate is rejected.

Fresh GNU-linked binaries from the unchanged normal compiler pass stable root
homes and Request signal under forced evacuation, evacuation+mark verification,
moving safepoints with 64 KiB scheduling, and budgeted old reclaim (8/8).
No shadow fallback, rooting budget reduction, or GC gate relaxation was used.

The prior accepted-code receipts remain applicable: serial runtime 5,007 passed/
5 ignored; codegen 1,977 passed/1 ignored plus integrations; fmt and gc_call_effects
green; GC matrix 79/80 with one equal-base pipe TypeError; dominance sabotage
40/40 caught; dependency native 1 within unchanged budget 3 and zero stale;
WASI zero hazards; lint base 26/head 25 inherited failures. Those complete suites
were not rerun for this scripts/docs-only follow-up.

## Witnesses and full bundle

Fresh GNU-linked q25/q50/q100/q200 function sizes are
22,172 / 44,477 / 89,478 / 175,935 bytes, retaining the q200 <= 248,858 gate.
Fresh compile timings are persisted in `sp-work/rss-linear-gnu/results.json`.
The unchanged mechanism's previous actual compiler sabotage gives q200
1,799,657 bytes and 48.61 CPU seconds; its size/map doubling gate rejects it.
No new sabotage compiler rebuild was needed because lowering is unchanged.

Fresh 15-pair micro execution comparison, with per-pair file warming and both THP modes:

| Witness | THP | Instructions % | CPU s base -> head | RSS KiB base -> head |
|---|---|---:|---:|---:|
| q25 | on | -0.582% | 0.01 -> 0.01 | 27,032 -> 25,444 |
| q25 | off | -0.583% | 0.00 -> 0.00 | 17,720 -> 17,780 |
| q50 | on | -1.057% | 0.01 -> 0.01 | 27,332 -> 25,452 |
| q50 | off | -1.047% | 0.00 -> 0.00 | 17,964 -> 17,740 |
| q100 | on | -1.910% | 0.00 -> 0.00 | 27,056 -> 25,496 |
| q100 | off | -1.893% | 0.00 -> 0.00 | 17,872 -> 17,924 |
| q200 | on | -3.463% | 0.00 -> 0.01 | 27,308 -> 25,952 |
| q200 | off | -3.434% | 0.00 -> 0.00 | 17,712 -> 18,012 |

All micro full/minor counts are zero. THP-off RSS medians increase by 60 / -224 / 52 / 300 KiB for q25/50/100/200; strict all-micro RSS acceptance is therefore also not established. q200 off has a paired median +112 KiB and descriptive bootstrap interval [40, 336] KiB. CPU values at 0.00–0.01 seconds do not resolve a strict timing delta.

The longer full Claude Code bundle retry succeeds on the immutable starting
head: exit 0, wall 37:23.84, CPU 5,759.44+141.89=5,901.33 seconds,
peak compiler RSS 37,133,896 KiB, executable 336,365,496 bytes.
The configured timeout was 10,800 seconds. All 128 LLVM units and final linking
complete. This is a compile receipt, not an application correctness claim.
The stripped executable cannot supply a separate nm-based GW7 measurement.
The previously extracted 56,506-byte real GW7 component remains
19,580,178 -> 740,053 function bytes and 103.04 -> 64.72 compile CPU seconds.

## Remaining owner decision

The original ~1.7 MiB RSS gap is not reproduced with equal cache residency.
All separate-arm RSS medians and all instruction/size rows are at or below base;
tiny RSS deltas remain statistically unresolved. **CPU all-at-or-below-base and THP-off micro RSS acceptance are
not proven.** No rejected prototype is in the bundle. This is the measured RSS
attribution and corrected measurement protocol, not a claim that every owner
acceptance condition has been satisfied.
