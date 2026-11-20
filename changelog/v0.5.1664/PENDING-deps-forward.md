Migrate the compiler and native TypeScript extension to one compatible SWC
dependency family: swc_common 26, swc_ecma_ast 29, swc_ecma_parser 45,
swc_ecma_codegen 32, swc_ecma_visit 29 and swc_ecma_transforms_base 49.
The extension now inherits the workspace versions. This removes the duplicate
SWC AST/visitor/transform stacks; it adds no caches, latches or side tables.

Port function and arrow bodies to `FunctionBody` / `ArrowFunctionBody`, and
object accessors to their backing `Function`, runtime-evaluated arrow bodies,
and JSX text to the new WTF-8 accessors. Keep the existing lowering,
strict-mode checks, closure scans and CommonJS require-shadowing diagnostics.
TypeScript's type-only `this` parameter is now separate from runtime parameters.
Preserve that annotation in inferred signature metadata while keeping it out
of runtime arity. Preserve syntactic parameter positions in native callback
recognition, widget extraction, declared FFI signatures, fixer diagnostics
and runtime evaluation. Keep existing optimization shape checks for functions
carrying a `this` annotation. A shared transient pattern
iterator reconstructs the old parameter view without persistent state.
No parser/HIR test expectations are regenerated.

Upgrade Perry's direct Brotli dependencies to 9.0.0, workspace thiserror to
2.0.18 and notify to 8.2.0. Older Brotli/thiserror versions required by external
transitive dependencies remain in the lockfile.

LLVM stays on llvm-sys 221.0.1 and LLVM 22. Migrating to llvm-sys 231 requires:

- LLVM 23.1 development libraries, headers and a matching `llvm-config`,
  configured with `LLVM_SYS_231_PREFIX`. The validation host has LLVM 21/22.
  Provision matching LLVM 23 on Linux, macOS and Windows build/release hosts;
  update the LLVM setup action, Windows import-library build script and LLVM
  version checks. Rebuild all compiler/runtime archives and validate native
  roots, stack maps, exception unwinding and performance on supported targets.
- A coordinated Inkwell upgrade/port with LLVM 23 support. Current Unix
  Inkwell 0.10.0 and Windows 0.9.0 select `llvm22-1`; changing Perry's direct
  llvm-sys dependency alone leaves Inkwell using incompatible LLVM 22 bindings.
- The [LLVM 23 changelog](https://releases.llvm.org/23.1.0/docs/ReleaseNotes.html#changes-to-the-c-api)
  replaces `LLVMBr` with `LLVMUncondBr` / `LLVMCondBr` and changes conditional
  branch operand order. Audit Inkwell's branch dispatch and any raw successor
  operand indexing; Perry's native-home CFG walk already uses `LLVMGetSuccessor`.
  The [231 bindings](https://docs.rs/crate/llvm-sys/231.0.0/source/src/core.rs)
  deprecate `LLVMIsConditional` / `LLVMIsABranchInst` in favor of the new branch
  predicates, correct `LLVMGetSyncScopeID` to return `c_uint`, and add byte types,
  byte constants and first-class denormal attributes. These functions are not
  directly called by Perry. Debug-location setting is renamed to
  `LLVMSetInstDebugLocation`; `LLVMAddMetadataToInst` becomes its deprecated
  alias. The changelog also changes printed floating-point IR literals to new
  formats, so audit Perry's NaN-boxed hexadecimal constants and IR assertions.

Validation used qb6 with LLVM 22, Node 26.5.1, separate main/forward targets,
CPUs 0–55 and at most eight Cargo jobs per lane. No builds ran on the Mac.
The required release build for `perry`, `perry-runtime`, `perry-runtime-static`
and `perry-stdlib-static` passed. `cargo fmt --all -- --check` and Node-version
consistency checks passed. Release checks also passed for
`perry-ext-typescript`, `perry-ext-zlib`, `perry-ext-parcel-watcher` and
`perry-container-compose`. Brotli, thiserror and notify needed no source API
changes at existing call sites.

| Dependency selected by Perry | Main | Forward |
| --- | --- | --- |
| swc_common | 18.0.1 | 26.0.0 |
| swc_ecma_ast | 19.0.0 | 29.0.2 |
| swc_ecma_parser | 32.0.0 | 45.1.2 |
| swc_ecma_codegen | 21.0.0 | 32.0.1 |
| swc_ecma_visit | 19.0.0 | 29.0.0 |
| swc_ecma_transforms_base | 32.0.0 | 49.0.1 |
| brotli (direct) | 8.0.4 | 9.0.0 |
| thiserror (workspace) | 1.0.69 | 2.0.18 |
| notify | 6.1.1 | 8.2.0 |
| llvm-sys | 221.0.1 | 221.0.1 |

The SWC extension already selected the newer family through bundler 56 and
loader 27. Unifying the compiler with it removes 24 packages from the first
commit's lockfile. Project/release version fields remain unchanged.

All four requested release test suites passed, with zero new failures.
Runtime tests used `--test-threads=1`; subprocess reruns printed inside a
parent runtime test are excluded from the totals below.

| Suite | Main passed | Forward passed | Failed | Ignored (both) |
| --- | ---: | ---: | ---: | ---: |
| perry-parser | 48 | 49 | 0 | 0 |
| perry-hir | 934 | 935 | 0 | 3 |
| perry-codegen | 2,561 | 2,561 | 0 | 6 |
| perry-runtime | 5,197 | 5,197 | 0 | 13 |

Supplemental CLI tests passed: six CommonJS require-shadowing tests and five
fixer tests. New coverage checks function-body lexical shadowing, the full
syntactic `this` parameter view and inferred signature metadata/runtime arity.

Both initial runtime runs had the same RSS-sensitive failure in
`gc::tests::native_payload_streams::z8_churn_releases_every_codec_at_completion_and_drops_every_payload`:
RSS growth exceeded its 4 MiB bound: 4,329,472 bytes on main and 4,333,568
bytes on forward (one 4 KiB page apart). All 4,000 codec creation/release/
payload-drop/finalization counters matched on both. Focused retries and full
runtime reruns passed on both arms; the table uses those full reruns.

No existing parser/HIR golden expectations were regenerated. All 1,444
corpus files have identical stable HIR hashes in main and forward: 1,436 gap
cases, three TSX cases and five additional typed-`this` cases covering
signature metadata, optimization shapes, stream callbacks, HTTP upgrades,
widget extraction and declared FFI signatures. Both arms used the same
ordered corpus in one process to preserve generated-name counter order.
There were no parse/lowering errors or panics. Five SWC codegen text hashes
change solely because the new emitter retains TypeScript getter annotations;
the corresponding lowered HIR is identical.

All eight eligible programs matched Node byte-for-byte on stdout in every
measurement and diagnostic run. The buffer driver compiled on both arms but
fails with the same `TypeError: value is not iterable` on both (known #12091),
while Node succeeds; it is excluded from the gate. The worker driver passes
on main despite its earlier reported #12092 status and is included below.

Measurements are medians of three interleaved runs, under
`flock /root/MEASURE.lock taskset -c 56-63 setarch -R`; each batch recorded
its lane/time in `/root/MEASURE.holder` and stayed below the batch limit.
RSS is `/usr/bin/time` maximum RSS in KiB. Full-collection counts come from
one separate fresh `PERRY_GC_DIAG=1` run per arm with identical inputs;
additional diagnostic ranges are discussed below. Cycles and all raw samples
are retained in the evidence files; instructions:u is the primary metric.

| Program | Main instructions:u | Forward instructions:u | Delta | RSS KiB main / forward | Full GC main / forward |
| --- | ---: | ---: | ---: | ---: | ---: |
| tsc | 26,971,543,047 | 26,972,253,149 | +0.0026% | 254,352 / 256,948 | 1 / 1 |
| zod5000 | 14,643,109,045 | 14,642,388,645 | -0.0049% | 52,388 / 52,364 | 0 / 0 |
| qsparse | 11,756,298,456 | 11,756,487,835 | +0.0016% | 57,280 / 57,600 | 0 / 0 |
| qsstringify | 30,013,229,279 | 30,013,286,070 | +0.0002% | 55,236 / 55,428 | 0 / 0 |
| commander | 7,544,427,975 | 7,543,075,455 | -0.0179% | 52,964 / 53,000 | 0 / 0 |
| hello | 1,347,831 | 1,347,755 | -0.0056% | 15,564 / 15,540 | 0 / 0 |
| fastify | 113,325,166,476 | 113,351,196,827 | +0.0230% | 257,820 / 258,512 | 1 / 1 |
| worker_heavy | 2,131,724,605 | 2,130,338,823 | -0.0650% | 230,440 / 220,264 | 41 / 42 |

The observed same-binary instruction spread (maximum minus minimum divided
by median) is:

| Program | Main spread | Forward spread |
| --- | ---: | ---: |
| tsc | 0.0455% | 0.0220% |
| zod5000 | 0.0427% | 0.0476% |
| qsparse | 0.0035% | 0.0024% |
| qsstringify | 0.0032% | 0.0289% |
| commander | 0.0278% | 0.0458% |
| hello | 0.0013% | 0.0011% |
| fastify | 0.1164% | 0.1404% |
| worker_heavy | 0.4966% | 0.9218% |

Every instruction delta is within the observed spread except hello's
76-instruction startup reduction. The notify/inotify/mio upgrade replaces
`inotify_init` with `inotify_init1` and removes the legacy `epoll_create`
import: hello has 229 GLOB_DAT relocations versus main's 230, with 15
JUMP_SLOT relocations in both. Fewer dynamic import bindings explain that
bounded startup change. No performance speedup is claimed for the noisy
programs, and there is no instruction regression outside the observed spread.

Tsc's original RSS median difference was +2,596 KiB. Per-process THP-off
checks used `prctl(PR_SET_THP_DISABLE)` and verified the disabled state plus
zero `AnonHugePages` in every sampled run, without changing host-wide settings.
The checks used three interleaved diagnostic runs per arm and sampled
`/proc/<pid>/smaps_rollup`; these sampled peaks differ from the primary
kernel maximum-RSS accounting and are reported separately.

| Program (THP off) | Sampled RSS KiB main / forward | Delta KiB | PSS anonymous delta KiB | PSS file delta KiB | Full GC main / forward (median) |
| --- | ---: | ---: | ---: | ---: | ---: |
| tsc | 213,836 / 214,648 | +812 | -108 | +880 | 1 / 1 |
| qsparse | 28,512 / 28,568 | +56 | -24 | +80 | 0 / 0 |
| qsstringify | 29,732 / 29,668 | -64 | -16 | -48 | 0 / 0 |
| worker_heavy | 108,280 / 105,316 | -2,964 | -2,732 | -168 | 41 / 42 |

Tsc's residual THP-off difference is +812 KiB and is file-backed code/data,
not an increase in anonymous memory or full collections. The new dependency
binaries change page placement and the set of resident code/static-data pages;
tsc's ELF text grows by 75,200 bytes and rodata by 131,030 bytes. Sampled file
PSS increases by 880 KiB, while anonymous PSS decreases by 108 KiB. Normal
runs also show roughly 2 MiB RSS/page steps. The qs checks similarly separate
small file-page/layout differences from THP effects: qs stringify's additional
normal diagnostic batch shows a 2 MiB AnonHugePages step that disappears with
THP disabled. The primary worker RSS ranges overlap (224,368–231,864 KiB main,
217,088–231,888 KiB forward); another normal diagnostic batch reverses the
median direction. Worker job interleaving changes per-worker live heaps and
collection timing; diagnostic full counts vary from 40 to 43 across runs.
Its RSS and instruction differences are scheduling noise, not a claimed
GC-regime improvement.

Tsc compile time is informational: the final forward rebuild took 520.61 s.
Main's recorded 1,210.70 s includes a deliberate pause and different build
contention/thread settings, so it is not a valid compile-speed comparison.
Compilation used `--no-auto-optimize --no-cache` and separate program caches.
The package drivers used TypeScript 5.8.2, Zod x5000, qs 6.16.0 (parse/stringify
10,000 iterations, 500 warmup), commander 15.0.0 (5,000/200), fastify 5.12.5
(5,000/300), and worker pool 4/jobs 400. `PERRY_ALLOW_PERRY_FEATURES=1` enabled
the approved package drivers.

The file-size lint still reports six violations that are byte-identical to
main; none is changed by this lane. All 38 changed Rust files are below 2,000
lines (largest: lower_module_fn.rs, 1,954). This unrelated lint failure is not
repaired here. Evidence and raw logs are saved under
`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/depsfwd-evidence/`.
The lane's Linux Cargo targets are removed after validation and bundle creation.
