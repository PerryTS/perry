# Lifecycle RSS attribution on current main

PR #12036 was rebased from `b49a915e2e9291e2e3c88fa33f1fa1f86710abbb`
onto `495fa8f94984177d945707a95ab0ef25aa4acf85`. This supersedes the older
base and measurements in `native-payload-lifecycle-validation.md`.

The reported +4–4.9 MiB comparison used an older main/merged tree. On current
main, the initial seven-pair comparison reproduced +1,584 KiB with THP off.
There is no corresponding extra heap allocation or retained payload in this
comparison. Clean executable page residency depends on the executable's
file-cache history. Disabling anonymous THP does not control those pages.

## Attribution

All measurements ran on qb6 in `/root/codex-lanes/cx-lifecycle-rss`, with main
in `main/src` and `main/target`, and head in `src` and `target`. Both arms built
the four requested packages with `cargo build --release -j 12`. The compiler
drivers used Node 26.5.1, `PERRY_ALLOW_PERRY_FEATURES=1`,
`PERRY_KEEP_SYMBOLS=1`, no auto-optimization/cache, module jobs 1 and codegen
unit jobs 2. No macOS builds, version edits or pushes were performed.

The large tsc generated objects were retained from main's complete native
build and relinked against each arm's separately built runtime/stdlib
archives. Relinking main reproduced the original **whole ELF byte for byte**
(SHA-256 `d583b52fb8aef44a9deb6e37975698a51d9f844bf86408b027116cd9f7414225`).
This controls generated code while measuring the lifecycle runtime change.
The other five drivers were compiled independently for each arm.

| Evidence | Main | Head |
|---|---:|---:|
| THP-off peak sampled RSS before equal cache preparation, median KiB | 182,596 | 192,792 |
| Anonymous pages in those samples, median KiB | 96,272 | 96,276 |
| File PSS in those samples, median KiB | 83,691 | 93,910 |
| Anonymous pages after equal preparation, median KiB | 96,708 | 96,724 |
| File PSS after equal preparation, median KiB | 83,404 | 83,322 |
| libc `[heap]` mapping resident KiB | 12 | 12 |
| Census reachable objects / bytes | 58,800 / 4,809,632 | 58,800 / 4,809,632 |
| Native-handle cells, live or dead | 0 | 0 |
| Arena capacity / occupied bytes | 63,963,136 / 46,871,336 | 63,963,136 / 46,871,336 |
| Eden / survivor0 / survivor1 / long-lived / old blocks | 32 / 1 / 3 / 2 / 23 | 32 / 1 / 3 / 2 / 23 |
| Mimalloc committed, one iteration | 144.9 MiB | 144.9 MiB |
| Mimalloc committed, three iterations | 191.1 MiB | 191.1 MiB |
| `mi_malloc*` requested bytes / calls | 113,925,585 / 2,325,609 | 113,925,631 / 2,325,611 |
| `mi_realloc*` requested bytes / calls | 35,213,491 / 163,871 | 35,212,723 / 163,870 |

Peak residency was sampled from `smaps_rollup` every 10 ms, retaining full
`smaps` at each higher sampled RSS. These sampled peaks and PSS values are
separate observations from `/usr/bin/time` maximum RSS; PSS also depends on
other processes sharing file pages. All THP-off samples had zero
`AnonHugePages`. Most anonymous memory belongs to mimalloc mappings, rather
than libc's small `[heap]` mapping.

The census is an explicitly requested full collection at the generated
program's `js_process_emit_before_exit_pending` call, after workload output;
it is an end-of-workload reachability observation, not a peak-RSS census.
Both GDB inferiors exited normally. An earlier console-entry breakpoint
experiment invalidated unrooted console arguments when forcing GC and is
excluded. Natural pressure diagnostics match arena sizes, promotions and
old-generation baseline; budgeted full cycles do not emit this synchronous
census.

All live type counts and bytes match: arrays 1,211,088; objects 332,840;
strings 1,641,152; closures 1,413,904; maps 6,960; lazy arrays 176; sets 560;
dates 48; object metadata 88,032; regexps 3,776; regex programs 43,208;
boxes 496; scopes 67,392. No per-object serial storage, larger cell or
per-thread allocation explains the gap. Cell size remains 136 bytes;
`GcTypeInfo` and the payload lookup hot paths have no layout/logic change.

The bpftrace probes matched `mi_malloc*` and `mi_realloc*`, with 12-frame
native stacks for requests of at least 1 KiB. Every request-size count at
1 KiB and above matches. Symbolized call-site differences merely redistribute
GC worklist/arena growth between scanner callers, with equal totals; the
combined requested-byte difference is −722 bytes. These traces compare the
initial rebased arm, before subclass integration corrections. Neither
correction is called by tsc. Raw traces and symbolized summaries are retained
with the delivered measurement evidence.

## Identical-binary control and measurement correction

Copying main's tsc ELF to a new file preserves the SHA-256 above but changes
its cache history. Seven interleaved pairs showed the following median RSS:

| Identical main ELF, KiB | Original file | Copied file | Difference |
|---|---:|---:|---:|
| Existing cache, normal THP policy | 221,724 | 232,868 | +11,144 |
| Existing cache, THP off | 181,944 | 192,544 | +10,600 |
| Equal cache preparation, normal THP policy | 221,960 | 221,272 | −688 |
| Equal cache preparation, THP off | 182,168 | 182,420 | +252 |

This is a cache-state effect on mapped executable pages. Reading each file
alone did not remove it; discarding its previous cached extents and reading
it identically did. No global `drop_caches` or system THP setting was changed.

`scripts/runtime_rss_ab.py` fsyncs each executable, applies
`POSIX_FADV_DONTNEED` only to that file, then reads it in 1 MiB chunks before
alternating arms. It records every sample and min/median/max and can check
against saved Node output. Its `--cache existing` mode supports the control.
THP off uses per-process `PR_SET_THP_DISABLE`; it does not alter the host.
The correction is in the measurement protocol. No speculative runtime
allocation optimization or allocator tuning was added to conceal the gap.

## Rebase integration

Current main added `alloc_with_prototype` and existing-object attachment for
ALS/AsyncResource. The rebase retains those constructors and gives the
existing-object operation the name `attach_to_object`, separating it from
the lifecycle operation `attach`. It roots subclass owners, reuses CLOSED
cells, rejects OPEN/CLOSING/finalized/incompatible cells and drops rejected
inputs. A new witness checks same-cell identity, the subclass's class id,
input destruction and finalized rejection.

AsyncHook already has an OPEN payload with an unpublished index at
`createHook`. Publishing the record must update that payload in place;
trying to replace it through attach correctly rejects OPEN and dropping the
rejected input retires the new record. The integration now initializes that
index in place, while a released hook reattaches through its CLOSED cell.
Existing moving-callback and worker-exit witnesses cover this case.

## Final verification

The final release build passed. tsc's text/data/bss sizes in bytes are
135,257,556 / 3,816,888 / 1,741,488 on main and
135,259,532 / 3,816,912 / 1,741,488 on head: +1,976 text bytes, +24 data
bytes and unchanged bss, far below the reported RSS difference.

Seven alternating pairs per mode, with equal executable-cache preparation,
give the following maximum-RSS distributions. The one-iteration rows are
the final repeat after our builds and tests stopped; the three-iteration
rows are the preceding final-build verification. Every output equals Node.

| tsc iterations / THP mode | Main min / median / max KiB | Head min / median / max KiB | Difference of medians KiB |
|---|---:|---:|---:|
| 1 / normal | 221,300 / 223,832 / 224,332 | 223,144 / 223,812 / 224,372 | −20 |
| 1 / off | 182,240 / 182,616 / 183,188 | 181,480 / 182,460 / 183,432 | −156 |
| 3 / normal | 266,420 / 268,556 / 269,476 | 266,292 / 268,712 / 269,316 | +156 |
| 3 / off | 223,296 / 223,544 / 224,232 | 222,808 / 223,496 / 223,972 | −48 |

All median differences and differences between the maxima in these final
distributions are within +0.5 MiB. The final one-iteration paired medians
are −144 KiB normally and −448 KiB with THP off. Variability remains:
the normal-mode mean difference is +579 KiB because three main samples
were about 2 MiB below its median. The preceding one-iteration verification
had mean differences +141 / +243 KiB and paired medians +320 / +316 KiB
(normal / THP off), but its THP-off difference of arm medians was +688 KiB.
These complete distributions are retained, rather than discarded when a
particular summary crosses a threshold. The old separation in which every
head sample exceeded every main sample is absent.

The final instruction check uses three alternating pairs per driver and
`instructions:u`; it includes the `/usr/bin/time` wrapper in both arms.

| Program | Main median instructions | Head median instructions | Difference |
|---|---:|---:|---:|
| tsc, one iteration | 10,193,434,762 | 10,193,275,524 | −0.0016% |
| Zod, 5,000 iterations | 16,117,308,383 | 16,115,492,209 | −0.0113% |
| qs parse | 28,552,609,568 | 28,559,870,464 | +0.0254% |
| qs stringify | 76,437,470,565 | 76,434,820,801 | −0.0035% |
| commander | 7,811,421,068 | 7,812,076,791 | +0.0084% |
| hello | 1,382,077 | 1,381,632 | −0.0322% |

`cargo test --release -j 12 --no-fail-fast -p perry-runtime -p perry-stdlib
-p perry-codegen -- --nocapture` completed with the following results:

| Package | Unit passed | Integration passed | Doc passed | Failed | Ignored |
|---|---:|---:|---:|---:|---:|
| runtime | 5,060 | 1 | 0 | 0 | 13 |
| stdlib | 247 | 0 | 0 | 0 | 0 |
| codegen | 1,996 | 501 | 3 | 15 | 6 |

Every T1–T12 and L4/L5/L8/L9 witness passes, and all 14 callback and four
lifecycle sabotages are RED. Same-cell subclass reopen and the existing
moving-callback / worker-exit hook witnesses pass. Cell layout remains
136 bytes and lifecycle churn retains its <4 MiB assertion and exact counts.

The requested all-zero codegen gate is **unfinished**: exactly the same
15 `native_proof_buffer_views` artifact assertions fail on untouched
`495fa8f949`. That baseline run has 32 passed / 15 failed. No codegen,
HIR, parser, manifest or lockfile source was changed by this lane; all other
codegen targets pass. These unrelated failures were not suppressed or
re-baselined to claim a green result.

Fmt, whitespace, Node-version consistency, native-handle ledger and GC
root-holder checks pass; root-holder self-test covers 92 planted shapes.
Inherited gates remain: `object/mod.rs` has 2,001 lines on both main and
head; raw-handle debt is identical at 869 sites and five module violations;
native-handle ledger self-test has the same stale ext-zlib classification.
No ceilings or inventories were raised.

All original comparisons, diagnostic replays, allocation traces, controls,
test logs and final verification samples are retained in the local
`lifecycle-rss/evidence` directory beside `lifecycle-rss.bundle`.
