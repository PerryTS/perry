# macOS ARM64 results — 2026-11-01

An incremental cold-code reduction for #13662: four installer attributes save
16,240 bytes in the scan and 16,256 bytes in the complete local chess application.
The long evaluation and search controls retain baseline steady-state performance.
The issue’s 4.43 MiB/original-speed combination remains unachieved.

## Controls

- Apple M1 Max, macOS 26.5, pinned `nightly-2026-10-04` (Rust LLVM 23.1.1).
- Base `ee0d9183d4496489dd4532247a64c66b57969230`; one freshly built compiler
  (application LLVM 22.1.8) for all arms. Candidate changes only four runtime
  `#[optimize(size)]` attributes and comments.
- Application O2, specialized ABI on, system allocator, runtime/stdlib O2,
  fat LTO, one codegen unit. Repository panic/GC/unwind configuration retained.
- Scan features: keepalive anchors and async bridge. The complete application
  additionally enables the regex engine in **both** arms; its source is unchanged
  and both builds use `--function-source header`.
- Fresh staticlib wrappers and fingerprinted GC-effects sidecars per arm.
  The benchmark build uses the issue’s two conservative forwarding-wrapper rule
  adaptations. An unmatched Arena teardown cut is omitted only when diagnosed.
- All experiment builds finished before measurement. Runs were serial, arm order rotated,
  with `taskpolicy -t 0 -l 0` (a scheduler hint, not CPU affinity). No sample was
  discarded. Byte counts are stripped, ad-hoc-signed executables.

Raw observations: [CSV](samples-macos-arm64.csv). Build flags, archive hashes and
static helper measurements: [provenance](provenance-macos-arm64.json).

## Public scan

Seven rounds per arm/mode, 5,000,000 probes after 100,000 warmup probes. Every
run returned warmup 42,188 and checksum 2,109,378. Inner time excludes setup and
warmup; instructions/cycles and process wall time include the complete process.
The Oz arm builds the candidate source with the diagnostic blanket-Oz profile.

| Runtime | Mode | File bytes | `__text` bytes | Inner ms | Process ms | Instructions (G) | Cycles (G) |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline O2 | forof | 5,650,896 | 4,874,832 | 315.348 | 338.367 | 5.389303 | 0.999551 |
| Baseline O2 | indexed | 5,650,896 | 4,874,832 | 303.089 | 318.714 | 5.250684 | 0.956760 |
| Cold installers / O2 | forof | 5,634,656 | 4,859,728 | 313.712 | 329.592 | 5.387727 | 0.992574 |
| Cold installers / O2 | indexed | 5,634,656 | 4,859,728 | 301.467 | 316.866 | 5.250311 | 0.956823 |
| Blanket Oz | forof | 3,369,792 | 2,557,328 | 501.502 | 523.759 | 5.976851 | 1.576008 |
| Blanket Oz | indexed | 3,369,792 | 2,557,328 | 489.948 | 512.219 | 5.854269 | 1.554911 |

The candidate removes 15,104 bytes of machine code. Blanket Oz is still about
1.60× slower for for-of and 1.63× slower indexed than the candidate O2 build.
Latest-main iterator work changes the mix from the issue’s original measurement;
do not carry its old absolute times or outlined-sample percentage forward.

### Static mechanism check

The three retained installer bodies shrink from 43,336 to 29,184 bytes, with a
new 24-byte closure body. Their callees and alignment contribute to the final
linked delta. Collection installation is inlined into the prototype installer.

| Hot helper | Baseline / candidate instructions in body | Baseline / candidate outlined references | Oz instructions in body | Oz outlined references |
| --- | ---: | ---: | ---: | ---: |
| `js_array_get_f64` | 469 / 469 | 0 / 0 | 227 | 48 |
| `js_array_length` | 518 / 518 | 0 / 0 | 269 | 59 |
| `js_array_record_stack_enter` | 176 / 176 | 0 / 0 | 97 | 29 |

These are static disassembly counts, not executed-call counts. The Oz comparison
changes the whole Rust optimization profile; it does not isolate the cost of
outlining from inlining and other optimizer decisions.

## Startup

31 paired rounds. `startup` forces builtin installation; `01_startup` prints one
line and does not retain the installers. Process time includes the counter tool
and process launch. These millisecond-scale wall times are sensitive to scheduling.

| Fixture | Arm | File bytes | Process ms | Instructions | Cycles |
| --- | --- | ---: | ---: | ---: | ---: |
| startup | speed | 5,668,496 | 8.252 | 36,242,524 | 13,550,430 |
| startup | cold | 5,652,272 | 8.121 | 36,288,741 | 13,554,377 |
| 01_startup | speed | 51,808 | 5.170 | 11,096,922 | 4,973,293 |
| 01_startup | cold | 51,808 | 5.218 | 11,108,137 | 5,100,747 |

Forced-initialization instructions increase 0.13%, cycles 0.03%; median wall
decreases 0.13 ms. The console-only executable has identical size and machine
code; its 0.05 ms wall-time difference illustrates the startup noise floor.

## Regular suite

All 17 numbered suite fixtures match the pinned Node 26.12.1 oracle, ignoring
only their declared measurement fields. Five rotated rounds per arm. Inner
timers report integer milliseconds; a one-tick change is 5% on a 20 ms test.

| Fixture | Baseline ms | Candidate ms | Instructions Δ | Cycles Δ |
| --- | ---: | ---: | ---: | ---: |
| 02_loop_overhead | 131 | 130 | -0.016% | -0.28% |
| 03_array_write | 22 | 21 | -0.505% | -0.66% |
| 04_array_read | 21 | 21 | +0.289% | +0.19% |
| 05_fibonacci | 521 | 517 | +0.010% | +0.05% |
| 06_math_intensive | 51 | 52 | +0.186% | +0.25% |
| 07_object_create | 4 | 4 | -0.336% | -5.69% |
| 08_string_concat | 6 | 6 | +0.006% | +0.31% |
| 09_method_calls | 36 | 36 | +0.132% | -0.19% |
| 10_nested_loops | 18 | 18 | -0.028% | -1.07% |
| 11_prime_sieve | 4 | 4 | +0.230% | +2.12% |
| 12_binary_trees | 5 | 5 | +0.060% | +1.71% |
| 13_factorial | 99 | 98 | -0.025% | -0.22% |
| 14_closure | 49 | 49 | -0.036% | -0.16% |
| 15_mandelbrot | 23 | 22 | -0.203% | -1.24% |
| 16_matrix_multiply | 20 | 21 | +0.028% | +0.30% |
| 17_loop_data_dependent | 227 | 227 | +0.061% | +0.12% |

All inner-time medians differ by at most one tick except Fibonacci (521 → 517
ms). Whole-process instruction medians differ by at most 0.51%. The CSV retains
process wall time, bytes and counters for every suite observation.

## Complete application

The local chess application’s private source and raw application output stay
local. These are contextual controls; the public scan is independently
reproducible. Complete executable: **8,631,216 → 8,614,960 bytes**; machine code:
**7,363,856 → 7,348,176 bytes**. The workload executable is 6,998,560 → 6,982,320
bytes, and the native test executable is 7,063,728 → 7,047,488 bytes.

Five rotated rounds, with matching Node outputs and unchanged board state.

| Workload | Arm | Inner ms | Process ms | Instructions (G) | Cycles (G) |
| --- | --- | ---: | ---: | ---: | ---: |
| evaluate | speed | 462.773 | 477.998 | 8.677221 | 1.462755 |
| evaluate | cold | 465.375 | 487.556 | 8.678525 | 1.464709 |
| legal | speed | 141.964 | 153.592 | 2.759156 | 0.458609 |
| legal | cold | 142.069 | 154.260 | 2.759086 | 0.458549 |
| perft | speed | 139.019 | 153.925 | 2.749539 | 0.459789 |
| perft | cold | 138.212 | 153.466 | 2.748963 | 0.459687 |
| search | speed | 5704.874 | 5931.676 | 105.836114 | 18.213292 |
| search | cold | 5703.070 | 5930.216 | 105.842685 | 18.231758 |

Inputs: evaluation 100,000; legal moves 10,000; perft depth 4; search 250,000
nodes. One evaluation observation and the adjacent legal-move observation have
large wall-time outliers without comparable instruction/cycle growth. Both are
retained. The initial evaluation median was +0.56%, so it received a longer
follow-up: seven paired rounds of **1,000,000 evaluations**, also in the CSV.

| Long evaluation | Inner ms | Process ms | Instructions (G) | Cycles (G) |
| --- | ---: | ---: | ---: | ---: |
| speed | 4612.357 | 4626.828 | 85.426328 | 14.258376 |
| cold | 4610.312 | 4642.187 | 85.426288 | 14.245119 |

Long evaluation is effectively unchanged (−0.04% inner time, −0.09% cycles).
Search is likewise unchanged (−0.03% inner time, +0.10% cycles). These results
support a small size saving at the existing steady-state performance, not a
claim of a general runtime speedup or a zero-regression guarantee on all machines.

## Correctness and coverage

- GC bootstrap unit test passed with default runtime features, single-threaded
  under `perry-dev` (debug info and incremental compilation disabled). It arms a
  collection and asserts both deferral during bootstrap and subsequent service.
- Nine targeted fixtures passed against Node 26.12.1 in **both** runtime arms:
  builtin name/length descriptors, builtin prototype chains, prototype changes,
  collection brand checks, invoke-EH paths, trampoline unwind, global builtin
  lookup rooting, iterator-drain rooting and spread-accessor rooting.
- The complete application passed its full native/Node test fingerprint and all
  nine CLI scenarios, including the full-strength search, in both arms.
- Candidate text parity: 95 commands and 23 whitespace characters. Both arms
  passed PTY resize and terminal restoration for quit, Ctrl-C, SIGINT and SIGTERM.
- `rustfmt --check` on changed Rust files and `git diff --check` passed.

The trimmed runtime cannot link the broad `test_globalthis_builtins.ts` fixture
because that fixture selects dynamic evaluation, Intl and Temporal installers.
It is not counted as a passing test. The Rust test harness also requires default
features: reduced-feature attempts failed on optional-module/realm-export links;
the successful bootstrap run uses defaults.

Only macOS ARM64 was measured. x86, Windows, Linux and the normal O3 shipping
profile have not been performance-qualified by this experiment.
