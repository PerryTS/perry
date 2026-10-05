# #12023 quiet arm64 follow-up: stopped by build-host disk guard

Performance acceptance remains open. No candidate was promoted, and native statepoint lowering is unchanged.

The coordinator's quiet M1 mini run used d3098e903 as base and a0037eeb3 as head. Its 15 pairs found qs CPU 1.730 → 1.750 s (+1.156%), with IQRs 1.725–1.730 / 1.750–1.755 s and head slower in 13/15 pairs. tsc and commander improved; Zod's 0.05 s timer resolution did not resolve a smaller change. Those are coordinator receipts in /private/tmp/cxq-mini, not a new run from this follow-up.

## Arm64 build stopped

The build Mac initially had about 19 GiB free. Two isolated worktrees at the exact coordinator commits excluded the 944 MiB benchmark directory through per-worktree sparse checkout. The release build monitored disk space every five seconds as well as before each build/compile. It stopped at 14.869 GiB free and terminated only the owned build descendants. Its own partial target occupied about 560 MiB. No compiler or workload executable was completed, so there is no new arm64 profile, disassembly comparison, or mini A/B.

Space subsequently fell to about 10 GiB due to other activity. The two owned worktrees and partial target were removed; no mini artifact was created. Other worktrees and the coordinator's receipts/inputs were left alone. An asynchronous request for additional disk headroom or another arm64 build host remains unanswered at report generation.

## Hello: measured startup relocation cost

On qb6, both arms were built for x86-64-v2 with identical shipping release package/feature graphs. Rust CPU selection used a command-local Cargo configuration; RUSTFLAGS was not exported. LLVM 22 generated the program for x86-64-v2. Every callgrind and native instruction run disabled ASLR with setarch, cleared ambient Perry/allocator knobs, set process-local THP off, and used executable paths of equal length.

Three repeated callgrind pairs are deterministic within each context. Unrebuilt native equal-path traces show +924 instructions, with +938 in _dl_relocate_object_no_relro. Fresh x86-64-v2 traces show +878 overall and the same +938 in that loader function. Ordinary RELA counts are equal (229 plus 15 PLT); packed RELR changes:

| ELF statistic | Base | Head |
|---|---:|---:|
| Relocated pointer locations | 31,387 | 31,392 |
| Packed words | 1,457 | 1,458 |
| Direct words | 11 | 9 |
| Bitmap words | 1,446 | 1,449 |

The runtime data layout changes the amount of loader bitmap work. The GC-map section is 77 bytes in both arms and has no dynamic relocations. This cost is not a stack-home load in hello's generated main. The normalized main instruction stream is unchanged. Relocation-address and pointee receipts are in arm-hello-callgrind-initial/relr-analysis.json; labels for strings can include adjacent string bytes, so that exploratory diff is not a definitive source attribution for each added pointer.

## qs Linux executable pages: static-size check

The retained Linux binaries from the preceding cycle audit have identical sets of 118 text symbols belonging to qs's parse/utils modules. This is a static-size comparison, not an arm64 hot-function profile:

| Bytes | Base | Head | Delta |
|---|---:|---:|---:|
| Whole .text | 16,410,482 | 16,297,586 | -112,896 |
| qs parse/utils text symbols | 543,274 | 490,334 | -52,940 |
| .rodata | 2,101,372 | 2,100,860 | -512 |
| .perry_gcmap | 180,256 | 165,704 | -14,552 |

Only five matched qs text symbols grow, all anonymous-shape constructors, totaling 1,731 bytes; none of the matched closure bodies grows. This rules out whole-module code growth as the explanation for the +348 KiB resident executable-page median. It does not establish which pages are faulted by hot arm64 execution or explain qs's CPU regression. Addresses and per-symbol deltas are retained in sp-work/arm-qs-linux-layout.json. The earlier peak-smaps receipts show unchanged anonymous/stack RSS and changed executable residency, so a layout/faulting issue remains open rather than a proven stack-frame RSS increase.

## Rejected storage prototype

A generic runtime callback-root prototype placed words in stable local UnsafeCell cells and registered their addresses through the already-existing runtime-handle scanner. Callback reads accessed those cells directly instead of decoding TLS handle indices. It added no root registry, shadow-frame fallback, platform selector, or name-based policy. The full serial release runtime suite passed (5,096 passed, 5 ignored; integrations passed).

The prototype reduced the loader delta from +938 to +126 instructions. Total callgrind counts also changed in pthread_getattr_np's map-scanning and sscanf/strtoul work; those are observer-sensitive because the guest sees Valgrind's mappings. Native perf instruction counts provide the independent control:

| Arm | Native instruction median (15 interleaved triples) | Delta from base |
|---|---:|---:|
| base | 1,203,649 | +0 |
| head | 1,204,581 | +932 |
| cell | 1,203,822 | +173 |

The prototype still exceeds baseline and does not satisfy strict instruction acceptance. It was rejected and the original source restored. The standalone candidate and experiment scripts/receipts remain under sp-work; no functional change was committed.

## Restored implementation verification

The original implementation was rebuilt coherently before these checks. Tested code remains at 7547283b4; the delivery commit adds only this audit and a changelog note. Compiler/archive identities are retained in sp-work/arm-restored-checks/identity.sha256. The JSON records exact statuses. Baseline-equivalent failures are kept explicit.

| Witness | Function bytes | Compile CPU s |
|---|---:|---:|
| q25 | 22,172 | 1.74 |
| q50 | 44,477 | 2.19 |
| q100 | 89,478 | 3.55 |
| q200 | 175,935 | 7.61 |

Serial release runtime: 5,096 passed, 5 ignored; integration test passed; eight doc tests ignored.

Native corpus: 252/252 sources compiled, no skips. Dominance: 7,564 functions / 289 modules, 71,144 statepoints; zero unrooted or stale hazards; all 40 seeded violations caught. Formatting passed.

GC matrix: 80 witnesses × four knob sets; 0 new failures; 0 stdout differences; 4 baseline-equivalent failing runs. The readable-from-pipe witness retains the same TypeError in every mode.

The previous sabotage receipt remains unchanged: disabling stable homes made q200 1,799,657 bytes and failed the size gate. No new sabotage build was run for this documentation-only follow-up.

## Remaining work

An arm64 build host with sufficient disk headroom is required to profile qs, attribute its hot-loop memory traffic, test a lowering fix, and perform the final quiet mini 15-pair CPU/RSS comparison. qs executable-page RSS and the remaining hello instruction delta also remain open. The Mac disk stop prevents completing these acceptance items. A fresh rebase and its corresponding validation/quiet measurements are deferred until a candidate can be tested; main was fetched but the measured source was kept at the exact frozen head. No push or PR mutation occurred.
