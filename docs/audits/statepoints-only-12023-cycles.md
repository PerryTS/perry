# #12023 cycles follow-up

Owner performance acceptance remains open. This report does not treat fewer instructions as proof of nonpositive CPU cost.

The lane was rebased onto the fetched, pinned main f969b820f69e904a9f04dde9d7f8dcc216938127 before fresh builds. Main advanced again during measurement; every new comparison retains that exact baseline. A newly added native constructor callback used the deleted shadow API: its five argument cells now use the existing RuntimeHandleScope::root_heap_word_cell, with scope drop before rethrow (eb2523f9b). A fixed-package GC-effects check also found two lane-added stale zlib callback classifications; fa9e083ff deletes them so unknown callbacks conservatively default to Reenters, matching the baseline and measured archives. No statepoint lowering policy, linker option, allocator option or GC gate budget changed.

qb6 could not sustain a quiet physical core. All-core mpstat surveys and guarded attempts are retained. Selected SMT pairs 30/62, 27/59 and 3/35 became busy before measurement. A pilot on 29/61 completed one on-mode pair per original workload before interference returned; it is not a 15-pair estimate. The resumable original campaign keeps the same core 29, rejects initial/pre-pair/sibling activity and low scheduled CPU share, and reruns both arms of any interrupted pair. It retains binary/oracle/harness identities and distinct raw receipts for every attempt. Both guarded campaigns exhausted 25 attempts with disposition quiet-core-unavailable. Original complete-pair count at report generation: 1.

Requested cycles:u, instructions:u and L1-dcache-load-misses:u work. Generic LLC-load-misses:u and stalled-cycles-backend:u are unsupported, not zero. No foreign PID, frequency setting, watchdog, shared page cache or global THP setting was changed. Own build jobs were initially confined to CPUs 8–15 and 40–47; the dominance checker was subsequently allowed to schedule outside 29/61. A quiet-host rerun is required for cycles/IPC and any conditional tsc cycles profile. No lowering fix was inferred from busy-host CPU seconds.

## Fresh five-workload comparison

15 pairs per workload and process-local THP mode, same core 30, final compiler including fa9e083ff, equal complete-file cache warming, exact saved pinned Node output. These are instruction and direct workload RSS measurements, not a quiet-core cycle acceptance run. The PMU execution and separate direct GNU-time RSS execution each retain their own GC receipts. CPU seconds in the direct-run receipts are busy-host observations, not the requested cycle comparison.

| Workload | THP | Instructions % | RSS KiB base → head | Paired RSS median [bootstrap95] KiB | Fulls / minors base → head | Binary bytes base → head |
|---|---|---:|---:|---:|---|---:|
| tscwork | on | -0.076 | 286,088 → 260,200 | -25580 [-26880, -23912] | 1/4 → 1/4 | 201,506,008 → 191,576,152 |
| tscwork | off | -0.070 | 242,720 → 220,796 | -21824 [-22880, -21640] | 1/4 → 1/4 | 201,506,008 → 191,576,152 |
| zodwork | on | -0.096 | 57,264 → 57,080 | -80 [-552, 180] | 0/4 → 0/4 | 25,042,464 → 24,849,424 |
| zodwork | off | -0.105 | 28,344 → 28,044 | -256 [-388, -68] | 0/4 → 0/4 | 25,042,464 → 24,849,424 |
| qs | on | -0.576 | 57,996 → 58,376 | +348 [200, 480] | 0/131 → 0/131 | 27,855,584 → 27,723,680 |
| qs | off | -0.567 | 30,392 → 30,740 | +288 [36, 392] | 0/131 → 0/131 | 27,855,584 → 27,723,680 |
| commander | on | -0.290 | 53,916 → 54,080 | +224 [-544, 360] | 0/33 → 0/33 | 34,749,312 → 34,707,664 |
| commander | off | -0.293 | 27,508 → 27,540 | +96 [-248, 176] | 0/33 → 0/33 | 34,749,312 → 34,707,664 |
| hello | on | +0.102 | 15,580 → 15,580 | +12 [-76, 28] | 0/0 → 0/0 | 19,090,784 → 19,085,608 |
| hello | off | +0.086 | 7,864 → 7,908 | +96 [-36, 520] | 0/0 → 0/0 | 19,090,784 → 19,085,608 |

New pinned-main results do not meet the all-at-or-below-base requirement: qs RSS increases in both THP modes, and hello instructions increase by about 0.09–0.10%. Commander RSS and several micro differences overlap noise; their small positive paired medians do not certify a nonpositive bound. All tsc counter runs have one full collection; its timing has not been accepted as a CPU regression or a win.

Five sampled peak smaps pairs for qs attribute the increase to executable-backed pages. THP-on anonymous RSS is 40,388 KiB in both, binary RSS 16,184 → 16,536 KiB; off anonymous 12,984 → 12,980 KiB, binary 16,116 → 16,724 KiB. Resident stack is 44 KiB and VmStk 132 KiB in both modes/arms. Promoted bytes 556,760 → 556,848; minors 131 in both. Receipts: cycles-qs-rss-sampling. A change to home retention or slot colouring is not supported by this RSS attribution.

Hello's generated main is instruction-for-instruction identical after normalizing addresses. Sixty instruction-sampling executions per arm are dominated by dynamic-loader and allocator startup. The profile has not isolated the small instruction increase; do not infer a root cause from those percentages. Receipts: cycles-hello-instructions-profile.

## q200 RSS correction and witnesses

The previous +300 KiB THP-off q200 RSS figure included perf's memory. GNU time wrapped the profiler process tree; perf itself dominates this short witness. Fifteen scope-control pairs show:

| Scope, THP off | Base median KiB | Head median KiB |
|---|---:|---:|
| direct | 11388 | 10024 |
| perf-wrapper | 17544 | 17604 |
| perf-only | 17516 | 17584 |

perf-only means perf running true, with no q200. Five final-call GDB snapshots support the attribution: stack 20 → 20 KiB, anonymous 1,732 → 1,720 KiB, binary-backed 7,824 → 6,808 KiB. Those are instrumented snapshots, not an uninstrumented peak claim. The harness now measures RSS in a separate direct execution. Its black-box test surrounds true with a fake profiler allocating 64 MiB and asserts workload RSS < 32 MiB; it passes. An interrupted-half-pair test also proves resumption repeats both arms and preserves previous receipts.

Fresh pinned-main witnesses:

| N | Function bytes base → head | Compile CPU s base → head | Head map bytes |
|---:|---:|---:|---:|
| 25 | 31,991 → 22,172 | 2.59 → 1.64 | 2,749 |
| 50 | 64,512 → 44,477 | 3.86 → 2.14 | 5,752 |
| 100 | 130,716 → 89,478 | 4.02 → 3.35 | 11,252 |
| 200 | 263,625 → 175,935 | 6.58 → 5.87 | 23,854 |

Head passes the 248,858-byte q200 bound and every 2.5× adjacent size/map bound. The baseline q200 263,625 B fails the bound, as expected for this pinned-main control. Compile times are busy-host receipts. Previous actual-lowering sabotage remains 1,799,657 B and 48.61 CPU seconds with a failing size/map gate; the lowering mechanism was unchanged in this follow-up.

| Witness | THP | Instructions % | Direct RSS KiB base → head | Paired RSS bootstrap95 KiB |
|---|---|---:|---:|---:|
| q25 | on | -0.576 | 25512 → 25344 | [-432, 216] |
| q25 | off | -0.569 | 9588 → 9844 | [-216, 344] |
| q50 | on | -1.064 | 25780 → 25488 | [-444, 0] |
| q50 | off | -1.027 | 9612 → 9576 | [-52, 16] |
| q100 | on | -1.875 | 25584 → 25572 | [-380, 400] |
| q100 | off | -1.906 | 9968 → 9920 | [-424, 132] |
| q200 | on | -3.411 | 25872 → 25896 | [-544, 212] |
| q200 | off | -3.454 | 10032 → 9964 | [-280, 136] |

Micro fulls/minors are zero in both the PMU and RSS runs. Corrected original-arm micro receipts separately show all RSS/instruction medians≤base; fresh pinned-main micro small differences overlap noise. Neither table preserves the erroneous perf-dominated RSS claim.

## Correctness and gates

Final-source release runtime 5096 passed/5 ignored; codegen 2012 passed/1 ignored, integrations and docs pass, fmt passes. GC 80-source matrix across force, force+evacuation+markverify, moving+sched64KiB, and instrumented budgeted reclaim has no new difference. Seventy-nine sources pass all four modes; stream_readable_from_pipe fails identically in both arms/all modes. Longer HTTP2 compile retry succeeds in both arms/all four modes. Merged baseline receipts: cycles-gc/merged-results.json and findings.json. After fa9e083ff, all 80 head sources were rebuilt and rerun in all four modes. Two HTTP-linked compiles initially hit a stale compiler/runtime identity check after wrapper rebuild; a coherent compiler/archive rebuild and explicitly instrumented retry resolved them. Final merged results and comparison against the exact baseline receipts are cycles-gc-final/{merged-results,comparison}.json. The rejected identity-mismatch attempts remain separate; the guard was not bypassed.

| Fresh gate | Exit |
|---|---:|
| base-effects-exact | 1 |
| base-gc-effects | 1 |
| base-lint | 1 |
| base-lint-node | 1 |
| base-native-corpus | 0 |
| base-native-dominance | 0 |
| head-effects-corrected | 1 |
| head-effects-exact | 1 |
| head-gc-effects | 1 |
| head-lint | 1 |
| head-lint-node | 1 |
| head-native-corpus | 0 |
| head-native-dominance | 0 |

Required fixed-package GC-effects checks and full lint/dominance comparisons are reported by their exact statuses and raw logs in cycles-gates. Both native dominance corpora pass with 252/252 sources and all 40 seeded hazards caught. Proper-Node lint has the same 20 failed commands in both arms, with no new failure. The exact GC-effects check has 344 inherited unsafe classifications in baseline and 342 in head, with no new unsafe symbol; the two resolved symbols are the removed native shadow APIs. These gates are not globally green. Exact failed-command and unsafe-symbol comparisons are in cycles-gates/comparison.json and the off-branch raw audit receipts. Pinned Node 26.5.1 was downloaded into a lane-only temporary directory and verified against the published archive checksum for the authoritative lint-node reruns; initial missing-Node lint receipts remain separate. A failure is never hidden or budget-relaxed. Instrument-profile preliminary GC-effects logs do not substitute for the fixed-package profile.

## Reproduction / handoff

Choose a physical core with mpstat and retain its SMT sibling checks. From the lane worktree:

```sh
python3 scripts/measure_linux_rss_ab.py --manifest /root/claude-lanes/sp-work/cycles-original-manifest.json --output /root/claude-lanes/sp-work/cycles-original-new-host --runs 15 --cpu IDLE_CORE --cycles
python3 scripts/measure_linux_rss_ab.py --manifest /root/claude-lanes/sp-work/cycles-final-macro-manifest.json --output /root/claude-lanes/sp-work/cycles-five-new-host --runs 15 --cpu IDLE_CORE --cycles
```

Use --resume after a rejected window. Remap absolute manifest paths when transferring binaries, saved oracles and required bench inputs to another x86_64 Linux host; binary SHA-256 receipts are in the JSON. No quiet-core guards should be bypassed.

The previously resolved full Claude Code bundle compile is unchanged: GNU normal-arm full link exits 0, wall 37:23.84, CPU 5901.33 s, compiler peak 37,133,896 KiB, executable 336,365,496 B. This is a compile receipt on the previous immutable implementation, not a post-rebase application parity claim.

No push or PR write is authorized or performed. Delivery refreshes the d3098e903..HEAD bundle after committing this audit. Open items are quiet-host cycles/IPC and conditional tsc profiling, qs executable-page RSS, hello startup instructions, and any inherited/new gate failures listed above. Owner performance acceptance is not complete.


## Retained cycle diagnostics

These complete tsc on-mode pairs are diagnostic only. Neither campaign reached 15 pairs for every workload/mode, so they do not establish a cycle regression or a win. LLC misses and backend stalls have no supported measurements.

| Campaign | Pairs | User cycles base → head | IPC base → head | L1 data-load misses base → head | Fulls |
|---|---:|---:|---:|---:|---:|
| original arms | 1 | 11,673,908,170 → 11,579,086,935 | 2.5895 → 2.6085 | 258,040,393 → 251,713,182 | 1 → 1 |
| pinned main | 1 | 11,322,121,905 → 11,301,745,763 | 2.6220 → 2.6237 | 258,902,669 → 267,984,731 | 1 → 1 |

Quiet-arm64 follow-up: [disk stop, hello relocation measurements and remaining acceptance items](statepoints-only-12023-arm64.md).
