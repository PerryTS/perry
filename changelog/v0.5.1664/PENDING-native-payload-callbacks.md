Add the runtime callback API for native payload owners (#11919).

- The malloc NativeHandle cell remains 136 bytes on 64-bit hosts. Its former finalizer hint
  is a NaN-boxed owner edge, visited by the shared mark/rewrite descriptor;
  its padding holds async refs, native-call nesting, PENDING and CLOSING.
- Callback families opt in with `links_owner`. Stable OwnerLink tokens and
  boxed CallbackSites keep C userdata valid while the owner moves. Async
  refs pin the cell, trace the owner, and never arm the young-pin latch.
- Native trampolines catch JS throws, retain the exact first exception in
  owner JS state, and suppress subsequent callbacks until explicit finish.
  Reentrant entries track busy; close while busy becomes immediately closed
  to JS and destroys the resource after the outermost C call returns.
- Finalization marks the cell before invoking native cleanup. Finalizing,
  closed and wrong-thread links return None without throwing or allocating.
  Runtime-owned callback state uses own data slots, avoiding inherited
  getters/setters inside C frames.
- Document S/A/C/N family classifications and conversion checklists. Existing
  crypto family declarations gain `links_owner: false` solely to compile the
  extended struct; no families are converted in this runtime lane.

Design clarification: the owner store uses
`runtime_write_barrier_external_slot`, because the proposed
`runtime_write_barrier_slot` does not remember malloc parents. T3 caught
this distinction. OwnerLink uses an inert usize representation (Send), and
NativeCallGuard is thread-affine with a zero-sized marker.
`set_pending_exception` additionally supports validation errors returned by
callbacks: families can park a TypeError without throwing through C. Receivers remain
rooted by the caller across the C span; finish precedes result conversion.
The 32-bit layout packs the finalizer into former alignment padding and the
bounded debug-name length into one byte, preserving the old cell size there
too; size invariants are compile-time assertions.

Runtime witnesses (fake native callers, without sqlite/net conversions):

| Design case | Witness | Sabotage, observed RED |
|---|---|---|
| T1 | owner moves during an active native call; stable userdata after 4096 sites | omit owner rewrite |
| T2 | two cell refs retain owner and callback across full GC without other roots; last unref releases both | omit owner mark; omit cell pin |
| T3 | malloc cell's young owner is remembered and rewritten | omit owner-store barrier |
| T4/T5 | exact thrown object, C regains control, reusable owner, first throw wins, no further JS | omit catch; bypass PENDING |
| T4 validation | park a TypeError without throwing through C; preserve its identity | same catch/pending witnesses |
| T6/T11 | explicit close, sweep, direct teardown, actual worker-thread teardown run no JS | ignore finalized check; delay finalized until after drop |
| T7/T8 | nested entry succeeds; close is visible immediately, native drop waits for outer finish | dispose while busy; deny nested entry |
| T9 | wrong-thread link returns None without a throw | use throwing thread validation |
| T10 | 200000 owners with JS-state owner cycles, no close; all finalized; RSS plateaus | leak a keep-alive ref |
| T12 | caught nested throw leaves busy=0 and try depth unchanged | throw result conversion before finish |
| latch | cell refs preserve copying eligibility, skipped preflight, and actual owner movement | pin young owner instead of cell |

All 14 sabotage children execute exactly one witness and exit unsuccessfully.
The focused suite passed 16/16. Its churn run finalized 200000/200000 owners;
warm RSS 60858368 bytes, peak 62984192, delta 2125824 (2.03 MiB).
Manual full GC is used in liveness fixtures to exclude the automatic
collector's conservative neighboring-block persistence. T2 also asserts
the mark bit before sweeping, so unreused stale bytes cannot hide a missing
owner trace. T8 covers runtime reentry, with family mutex removal left to the
sqlite lane. T10 models closure capture with the equivalent strong JS-state
cycle. Real family callback/output tests remain in their later lanes.

Linux A/B, perrymaster, separate main/candidate targets and caches, normal
release profile, `PERRY_NO_AUTO_OPTIMIZE=1`, Node 26.5.1. Three alternating
runs per arm after one warm-up; medians below. `perf instructions:u` counts
userspace instructions, including the small identical `/usr/bin/time`
wrapper. CPU is perf task-clock, RSS is the executable's peak KiB, and fulls
include synchronous and budgeted full collections under `PERRY_GC_DIAG=1`.

| Program | Instructions main → API | Delta | CPU seconds main → API | RSS KiB main → API | Fulls |
|---|---:|---:|---:|---:|---:|
| hello | 1342530 → 1342265 | −0.020% | 0.014046 → 0.023415 | 6780 → 6892 | 0 → 0 |
| Zod 4.6.5 | 223003657 → 223031820 | +0.013% | 0.310888 → 0.223670 | 30412 → 30596 | 0 → 0 |
| TypeScript 5.9.3 | 2549501999 → 2550015030 | +0.020% | 0.868477 → 0.802300 | 121072 → 131324 | 0 → 0 |

Every measured stdout matched Node. CPU/RSS are noisy samples, not evidence
of an improvement or a regression. The initial kernel-inclusive pass is
also preserved: median instruction deltas hello +1.410%, Zod −0.028%, tsc
−0.055%; first hello samples exceeded subsequent samples by over 2×, hence
the explicit warm-up and userspace counters. No measurement was discarded.
Perf emits raw nanoseconds for unitless task-clock CSV on this host; the
saved parser and numeric artifacts account for that.

Driver deviation: qb6 and qb6.skelpo.net do not resolve from either the Mac
or perrymaster. Use the repository tokio-removal hello probe, release Zod
fixture, and a driver importing the full TypeScript compiler and repeatedly
type-checking 100 functions with an intentional type error (Node: `tsc 5`).
The exact sources, package lockfile, raw perf/time/diagnostic logs, initial
and warmed measurements, and reproduction scripts are saved beside the
bundle in `scratchpad/codex-small/`. The original qb6 drivers were unavailable.

Static validation: GC call-effects lint passes on Linux/macOS/Windows;
its 9-helper × 3-format sabotage self-test passes. Root-dominance self-test,
poll-reach and macro item-position audits pass. Debt/ledger no-raise checks
against 4c87719783bc7558377e736714d1d6770619aaba pass: debt ceiling 868 stays
868 (actual sites 863), ledger tables 207→207 and producers 188→188.
Their strict checks are already red on the supplied main and remain so:
`thread/clone_read.rs` has two debt sites without a module ceiling;
`perry-ext-zlib/src/stream/agent_state.rs` and
`perry-stdlib/src/zlib/tables.rs` each have one table against a zero ceiling.
No inventory or ceiling is widened by this lane.

Additional repository checks also have baseline failures: root-holder
inventory has existing uncovered/stale entries and stale census source pins;
the file-size checker flags five unrelated files above 2000 lines. All
modified files stay below 2000. These pre-existing issues are left outside
the runtime callback scope. The 32-bit size assertions are provided but were
not cross-compiled on this 64-bit host.

Final test counts:

| Target | Passed | Failed | Ignored |
|---|---:|---:|---:|
| runtime unit | 4931 | 0 | 5 |
| runtime integration | 1 | 0 | 0 |
| codegen unit | 1988 | 0 | 1 |
| codegen integration (43 targets) | 516 | 0 | 0 |
| stdlib unit | 239 | 1 | 0 |
| doc tests | 3 | 0 | 13 |

The seven stdlib container integration targets run zero tests under the
requested default feature set; their binaries also exit successfully.
The stdlib failure is
`runtime_thread_exit_tests::symbols_tests::thread_exit_releases_the_threads_dom_exceptions`:
the dead worker's address still reports a DOMException brand. It reproduces
alone, on a complete candidate stdlib rerun, and on the supplied main with
the same runtime/stdlib/codegen feature union. The logs retain all runs.
Therefore the requested zero-failure stdlib gate is **not met**; this existing
DOMException issue was not changed outside the callback API scope.

The new latch witness initially failed only its ancillary no-malloc-sweep
assertion in the full suite, because it requested a MallocCount-triggered
collection. It now uses the neighboring latch fixtures' Direct trigger and
registers the shape scanner. The complete runtime rerun passes, including
the latch baseline and all 14 RED sabotage children.

Root-dominance corpus: both candidate lowerings compile 248/248 sources,
zero skips, 283 LLVM modules. Shadow checks pass: 7434 functions and 33638
root stores, zero moving dominance violations; 25239 GC-capable allocas,
zero moving unrooted-alloca violations; zero stale-register uses. Main's
shadow moving checks also pass. Native corpus generation uses production
LLVM 22 rewrite passes and emits 69698 statepoints with 51408 nonempty live
bundles. The unfiltered shadow checker on main reports 26 nonmoving
violations; the CI moving-only modes above are the gates used here.
Both release builds and final `cargo fmt --all -- --check` pass.
Native moving check passes: 69415 parsed safepoints, 51408 live bundles,
141043 relocates, 131256 (safepoint, root) pairs; zero unrooted and zero stale
hazards. Its 40 planted violations are all caught, zero missed.
