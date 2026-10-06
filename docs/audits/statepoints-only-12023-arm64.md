Latest acceptance receipts: [main 77071e3d9 acceptance pass](statepoints-only-12023-acceptance.md). Acceptance remains blocked by the explicitly recorded performance rows.

# #12023 sequential arm64 follow-up: commander acceptance remains blocked

The retained implementation uses linear native statepoint homes and ordinary SSA statepoints for bounded root sets. Shadow-frame selection remains limited to platforms without native stack-map readers. The five follow-up prototypes are excluded from delivery because none met CPU and RSS <= base across all measured workloads.

## Matching quiet measurements

Both SDK arms were built sequentially on the M1 Max build Mac, with explicit compiler/runtime/workspace paths and an 8 GiB disk floor. Each target was deleted before building the next arm; follow-up experiments rebuilt only head. Completed binaries were retained separately. Every workload output matched Node 26.5.1. The mini ran measurements only.

Base 22ff275dad7d9b599d29483772ec4b9576dd04ed versus measured head 37e3aa0a2b05ba7cf229f3bab54d0bf5709a97ba: 15 interleaved pairs per workload, all load averages below 1.81 (maximum 1.60). CPU is user + system time; RSS is median maximum RSS. Tsc diagnostics show 3 copying minors and 0 full-cycle starts/completions on both arms.

| Workload | CPU seconds, base -> head | RSS MiB, base -> head |
|---|---:|---:|
| qs | 1.53 -> 1.51 | 22.953 -> 22.828 |
| tsc | 0.74 -> 0.73 | 161.625 -> 151.875 |
| commander | 0.52 -> 0.53 | 25.297 -> 25.469 |
| zod | 0.04 -> 0.04 | 21.344 -> 21.328 |

Commander remains + 1.92% at the short-run median. Fifteen longer pairs (50000 iterations of the same binary) give 4.92 ->4.98 seconds (+ 1.22%), confirming the increase beyond the short timer's 10 ms resolution.

## Measured root-cause evidence

Five interleaved sample pairs each for qs and commander were collected on the mini. Symbolized qs copies and the head commander copy have identical executable sections and addresses to their corresponding stripped measurement binaries. For stripped base commander, unique instruction-prefix matches and Mach-O function starts resolve 18,382 functions; ambiguous/unmatched addresses remain unknown. Disassembly comparisons use actual executable function boundaries.

Commander parseOptions shrinks from 47,028 to 39,372 bytes, its frame 576 ->560 bytes, and static stack-access instructions 1,603 ->613. Self samples 15 ->12 out of 3,799 each. The sampled regex VM run, native dispatch tower, and regex replace functions have unchanged function sizes, frames and stack-access counts. These facts do not support a larger-frame explanation. They do not prove cache stalls.

Peak commander vmmap at 0.25 seconds shows executable text 7,824 ->7,888 KiB; stack 48 KiB both, with matching heap allocations. Final maximum-RSS medians vary between repeated comparisons but remain above base. Extended GC diagnostics match 563 copying-minor entries, arena_live 445,144 bytes and capacity 8,388,608 bytes. GC minor time 79,288 ->78,815 microseconds in this diagnostic pair. The remaining lead is executable-page access/layout; a complete causal fix has not been demonstrated.

Qs parse18/parse22 keep identical frames and static stack-access counts. Parse 15 shrinks 8,900 ->7,576 instructions,1,219 ->485 stack accesses and frame 1,040 ->976 bytes. Quiet CPU and RSS are below base. Latest Linux executable-page sampling also removes the previous +348 KiB increase.

## Rejected prototypes

The attached JSON records source SHAs, fifteen-pair medians/IQRs and load checks.

- Entry-initialization proof: 15 native tests and 2,042 full codegen unit tests pass (one ignored); q gate passes. Commander still 0.52 ->0.53 seconds.
- At most 8  repeatedly-read loop values in ordinary SSA: 16 native tests and 2,043 unit tests pass (one ignored); q gate passes. Commander CPU ties but RSS rises; qs CPU rises 1.53 ->1.54 and Zod RSS rises.
- At most 1  hot loop SSA value: 16 native tests and q gate pass; clean repeated quiet comparison ties CPU but commander and Zod RSS rise. A first run overlapped tsc diagnostics in two qs pairs, was stopped and discarded, and the whole comparison was repeated without overlap.
- Bounded ordinary SSA root set 12: small-workload instructions decrease on qb6; qs CPU/RSS improve on the mini, but commander CPU remains higher and Zod RSS rises. Three native-test assertions also fail; this variant is not accepted or included.
- Coalesce private home stores between reads/collections: 16 native tests pass, including observation-boundary cases. Qs CPU improves and Zod ties, but commander CPU/RSS remain higher.

No padding, name-based exception, registry, GC-gate weakening or shadow fallback was introduced.

## Hello relocation fix and witnesses

On the matching 22ff/37 pair, semantic RELR locations 30,571 ->30,554; encoded words 1,447 ->1,446; RELA/PLT 244 unchanged. The removed locations came from redundant private integer sort-buffer bounds panic records. Public bounds and run checks remain; private cursor/slice invariants justify raw accesses, including a single mutable base pointer for overlapping copies.

Three deterministic callgrind pairs: total 1,413,829 ->1,413,399 (-430 Ir); loader 529,374 ->529,034 (-340). Fifteen direct native perf pairs: 1,198,940 ->1,198,593 (-347 instructions). Miri guarded-buffer merge and gallop-boundary tests pass.

Matching q25/q50/q100/q200 function bytes:

| N | Base | Retained implementation |
|---:|---:|---:|
|25|27688|18122|
|50|54824|36362|
|100|109397|72862|
|200|226786|138767|

The size gate passes. Disabling native homes while keeping statepoints makes q200 1,780,525 bytes; the size gate fails, detecting quadratic regression. Fifteen q200 RSS pairs per THP mode show off 10,064 ->9,912 KiB, on 25,964 ->25,548 KiB: the previous +300 KiB increase is absent.

## Verification and rebase scope

On the matching 22ff/37 source snapshot: instrumentation-verified 88 GC witnesses x4 knob sets have no stdout/exit/verifier differences; two existing stream failures match base. Serial runtime 37: 5,141 pass,0 fail,5 ignored. Matching base: 5,137 pass. Codegen and fmt pass on both component arms. Eight static GC gates pass. Native root-dominance corpus/check passes,40/40 planted violations caught. Lint exits 4 on both arms due to inherited workflow-extraction errors; it is not claimed green. WASI 37 cross-build and 6 smoke programs pass.

The full Claude Code bundle previously compiled successfully on 218499ffd: 34:20.74 elapsed, maximum compiler RSS 33,682,648 KiB, exit 0. This is a compile-only receipt, not a run or a check of the final rebase.

The retained 18-commit change set rebased cleanly onto current main bffd54bc240178d472ba0fa6828591acaa1a0c8e, yielding validation snapshot 43985a8efc7a1be706e184015fcb15125c93c529. Native-home source/tests are byte-identical to measured 37; newer main includes RegExp read-site changes. Fresh Linux checks are recorded separately in the lane progress note. Quiet performance has not been remeasured against the new main snapshot, and commander acceptance remains open.

## Fresh Linux checks after rebasing

Base bffd / head 43985: serial runtime 5,143 / 5,147 passed, zero failures, five ignored each. All 88 GC witnesses in four knob sets on both arms (704 runs) have no stdout/exit/verifier differences and every binary contains the requested instrumentation. The same two stream failures remain on both arms. Fmt and all eight static GC gates pass on both arms. Lint exits 4 on both arms with the inherited workflow-extraction failures.

Both full codegen suites pass with --test-threads=1. The initial parallel head run fails one ConstFn finalizer test; isolated runs pass on both arms. Three additional parallel unit repeats pass on base; head exits 101/0/0, with the failed repeat reporting a different ConstFn test. The failing test sources and ConstFn implementation are identical to base. An environment switch used by these tests is temporarily disabled by a concurrent test; the initial failure is preserved rather than hidden. No test or gate was weakened.

Strict gc_call_effects exits 1 on both default-feature SDK arms. Comparing the complete freshly generated tables finds 343 unsafe-drift entries on base and 341 on head, no new unsafe entry and identical classes for every shared entry. The two removed entries are js_shadow_slot_bind and js_shadow_slot_set. This is a baseline gate failure, not a green result. Checks against the earlier expanded/instrumented SDK are also preserved.

Fresh witness sizes remain 18,122 / 36,362 / 72,862 / 138,767 bytes; the q-size gate passes. The fresh dominance checker passes: 7,745 functions / 300 modules, 72,830 safepoints, 54,343 nonempty live bundles, 269,416 relocates, zero unrooted or stale hazards, no immovable-source exemptions, and 40 planted violations caught with zero misses.

During validation, origin/main advanced by three runtime class-read commits to 77071e3d9f589605ce44511265366e2db9cbb68f. The retained change set was rebased cleanly again, yielding 80834d84970fe211233777ac3ad12d8667dd77cf before the report commit. The entire codegen source tree is byte-identical to the validated 43985 snapshot. The additional upstream changes affect runtime class reads and their tests. Runtime and quiet performance were not rerun after this last rebase; the fresh Linux runtime/GC/call-effects receipts above apply to bffd/43985. This limitation remains explicit while acceptance is blocked.

All eight static GC gates also pass after the last rebase. Mac and mini build/measurement artifacts and all lane target directories are removed; scripts, results and verified binary archives remain on qb6.
