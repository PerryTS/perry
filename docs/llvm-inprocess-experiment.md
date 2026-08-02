# In-process LLVM backend experiment (`exp/llvm-inprocess`)

**Status: Phase 0 complete (transport, byte-identical) and Phase 2 landed
opt-in (native construction — module-scale IR text no longer exists under
`PERRY_LLVM_INPROCESS=native`).** The default path is untouched: the default
build links no LLVM and compiles `.ll` through `clang -c` exactly as before.

## Phase 2: native construction (`=native` / `=diff`)

The owner's directive after Phase 0: the lever is not the transport, it is
making generated `.ll` text *optional*. Two audits drove the design (both
reproduced below): 96.4% of instruction emission (6,097 call sites) already
flows through ~40 semantic methods on `LlBlock`; the bypasses are 89
`emit_raw` sites, the `LlFunction` entry-splice mechanism (136 sites), and
`to_ir`'s post-render passes (ret rewrites, setjmp volatile promotion). A
grammar census over two real corpora (9,110+ instructions) bounded the
dialect: ~30 opcodes, scalar + `[N x T]` types, six operand forms — plus two
traps a naive builder dies on: **non-phi cross-block forward references**
(the `idispatch` tower) and phis where 73% of incomings are forward.

Architecture (deliberately different from the brief's instruction-level
builder-trait): the port point is the **finalized function**, not the emit
stream. `LlFunction::to_ir()` output — including entry-alloca hoists,
boundary splices, return-site rewrites, and volatile promotion, so those
transforms keep exactly one implementation — is consumed per function by a
bounded dialect reader (`dialect.rs`) that constructs it through the C API:
typed operand resolution against a per-function `%name` map,
placeholder-RAUW for forward references (LLParser's own strategy), deferred
phi incomings, callsite-derived call types (opaque-pointer semantics,
measured: a direct call's function type is the *callsite's*, not the
declare's), pre-declaration of all module-internal defines. Only the module
*skeleton* (globals, declares, attribute groups — a few KB) is still parsed
as text. Per-function text remains transient scaffolding (dropped
immediately); the follow-up that removes it is typed `LlInst` variants
inside `LlBlock` behind the same reader interface, migrating the ~40
semantic methods opcode-by-opcode with `=diff` as the gate.

Results so far:

- Corpus gate (unit tests `dialect::tests::corpus_*`): every function of
  both corpora constructs natively and passes the LLVM verifier.
- `spike.ts`: text-parsed arm and natively-constructed arm emit
  **byte-identical objects**; the native-mode binary matches the pinned Node
  oracle.
- `batch.ts`: all pre-opt divergences are one class — the C-API builder
  constant-folds at construction (`zext i1 false`, `select i1 false`,
  constant GEPs) — which is why `=diff`'s verdict is **emitted object
  bytes**, not pre-opt prints. The arms' objects differ by 336 bytes of
  *tighter* native-arm code (register-allocation divergence downstream of
  pre-folded input; same symbols, verified at the disassembly level to be
  layout/regalloc, not semantics).
- Behavior: 26-test gap-corpus slice under `=native` (one binary, flag-only
  A/B, liveness asserted): first pass 25/25 byte-identical outputs with one
  compile failure — a multi-module test that exposed two reader bugs
  (explicit `external` linkage keyword; skeleton globals referencing defined
  wrapper functions, fixed by parsing the skeleton with synthesized declares
  for every define). After the fix the failing test compiles natively with
  identical output; the full slice re-run is recorded in the tracking issue.

Modes: `PERRY_LLVM_INPROCESS=1` transport (parse whole text in-process),
`=native` construction, `=diff` both-arms harness (returns the text arm's
object; prints `[ir-diff] OK/MISMATCH`; `PERRY_LLVM_DIFF_DIR` dumps both
arms' pre-opt IR + objects). All values share the env var already keyed into
both caches. Unit-split and `emit_ir_only` paths fall through to transport.

Companion artifacts:

- `experiments/llvm-inprocess-spike/` — the Phase 0 spike binary (see below).
- `crates/perry-codegen/src/inprocess.rs` — the integrated backend
  (`llvm-inprocess` cargo feature).
- Tracking issue: PerryTS/perry#7241 (Phase 0 answers in the opening post).

## Thesis (from the experiment brief)

Perry builds LLVM IR as strings, writes a `.ll`, and shells out to a
user-supplied `clang -c`. That is an LLVM dependency that is *runtime,
unpinned, and owned by the user's machine*: 1,376 MB of transient IR text on
the Claude Code bundle, the #4880 whole-module `-Os` demotion, Apple clang 21
unable to parse LLVM 22 attribute output, and errors reported as line numbers
in gigabyte files. The experiment: own the pipeline via the LLVM C API, make
textual `.ll` a debug view instead of the transport.

## Phase 0 answers (the questions the brief required)

**Which crate?** `inkwell 0.9.0` (safe wrapper) with `llvm-sys 221` alongside
for the C-API calls inkwell does not wrap. inkwell's typed builder API held up
for real construction work, and its `Context::create_module_from_ir` +
`run_passes` + `write_to_memory_buffer` cover the whole transport path with no
disk I/O. Raw `llvm-sys` alone would also work but buys nothing at this stage;
the escape hatch composes cleanly (`AsValueRef`), so this is not an
either/or.

**Which LLVM version?** 22 (`llvm22-1` feature / `llvm-sys 221.0.1`), matching
Homebrew's current `llvm` formula (22.1.4 on the dev box). Choosing 22 rather
than 20 *eliminates* the version-skew problem from the brief: the
statepoint/RS4GC work needs LLVM 22 output that Apple clang 21 cannot parse —
in-process, there is no parse and no second toolchain. inkwell 0.9 supports
llvm11-0 through llvm22-1, so pinning back is trivial if needed.

**How is it discovered at build time?** `llvm-sys` reads
`LLVM_SYS_221_PREFIX` (or falls back to `llvm-config` on `PATH`). The build on
this branch was done with `LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm`.

**What must a contributor install?** Nothing new, by default. The
`llvm-inprocess` feature is off; the default build has zero LLVM link
dependency and byte-identical behavior. A contributor who wants the in-process
backend needs `brew install llvm` (macOS) / LLVM 22 dev packages (Linux) and
the env var above. Static-link cost is real (see the binary-size row) and is a
distribution decision to make only if this graduates.

**Could the bindings express what Perry needs?** Yes — everything on the
brief's worry list, verified by construction in the spike's `demo` mode:

| Construct | Result |
|---|---|
| NaN-box constants (`0x7FFC_0000_0000_0001` payloads in `double`) | `f64::from_bits` → `const_float` preserved bit patterns exactly, verified end-to-end by running the compiled artifact |
| `f64` ↔ `i64` bitcasts, tag-check ladders | plain `build_bit_cast` / icmp; no friction |
| Inline asm with exact constraint strings (`call void asm sideeffect "", ""()` — Perry's loop barrier) | `context.create_inline_asm` + `build_indirect_call` |
| Module-level asm | `module.set_inline_assembly` (symbol verified present in the emitted object) |
| Appending globals / `@llvm.used` (the Mach-O no-dead-strip mechanism) | `Linkage::Appending` + `llvm.metadata` section |
| `gc "statepoint-example"` function attribute | not wrapped by inkwell — one-line `llvm_sys::core::LLVMSetGC` via `AsValueRef` |
| `-mllvm -inlinehint-threshold=N` passthrough | not a C-API option — `LLVMParseCommandLineOptions` (process-global; see Constraints) |
| Varargs extern declarations (runtime helper calls) | `fn_type(..., true)` |

Notably, `main`'s generated IR needs *less* than the brief feared: no
`llvm.used`, no module asm, no statepoints today (those live on
`exp/stackmap-viability`). The empty-constraint asm barrier is the only asm.

## What was built

### 1. Spike binary (`experiments/llvm-inprocess-spike/`, standalone workspace)

- `demo` mode: builds the table above through the builder API, verifies, runs
  `default<O3>`, emits a `.o`, links with `cc`, executes, and checks the
  program's own output (including the NaN-box bit patterns).
- **clang-shim mode**: accepts the exact argv Perry's
  `build_clang_compile_plan` produces and compiles in-process. Because both
  Perry caches already key on `PERRY_LLVM_CLANG`, pointing that variable at
  the shim A/Bs the whole backend through an **unmodified perry** with no
  possibility of cross-arm cache reuse. Liveness is proven by a per-compile
  log (`PERRY_LLVMC_SPIKE_LOG`), not inferred from the absence of errors.

### 2. Integrated transport backend (`llvm-inprocess` feature)

`compile_ll_to_object` in `perry-codegen/src/linker.rs` — whose own doc
header has always called it "the seam" — gets a second implementation:
parse-from-memory → verify → `default<O_n>` → `TargetMachine`
object-to-memory. No `.ll` touches disk (KEEP_IR and on-failure debugging
still write one). Decision parity is by construction: the in-process path
interprets the *same* plan argv the clang path builds (`-O3`/`-Os`/`-O0`
incl. the #4880 oversized fallback, `-mcpu=native`, inlinehint), so the
backends cannot drift on a decision without the plan's own tests catching it.
Unknown flags are a hard error, never silently dropped. `PERRY_LLVM_INPROCESS`
joins both the build-cache and object-cache keys (vacuous-A/B defense), a
build without the feature **fails loudly** if the flag is set, and the first
in-process compile prints a liveness witness:
`perry: in-process LLVM backend active (LLVM 22.1.4)`.

## Differential parity results

**Object-level:** on the same 245 KB Perry module with identical flags, the
in-process pipeline produced an object **byte-identical** to Homebrew
`clang -c` (LLVM 22). Apple clang 21's object differs (8562 vs 8554 bytes of
`__text`) — that is precisely the cross-version skew the in-process backend
removes from user machines.

**Corpus A/B (through unmodified perry, via the shim):** every 6th gap test,
77 tests, both arms compiled by the same perry binary and same runtime `.a`:

- 76/77: stdout, stderr, and exit status byte-identical.
- 1/77 (`test_gap_http_res_socket_writable_onfinished`): both arms SIGABRT
  with the same pre-existing `perry-ext-http` panic; the only stderr delta is
  the OS thread id, which also differs between two runs of the *same* arm.
  Not backend-attributable.
- 0 compile failures on either arm; 82 in-process compiles proven live via
  the log.

**Corpus A/B (integrated backend, one binary, flag-gated):** a different
26-test slice (stride 18, offset 3) through the same perry built with the
feature, `PERRY_LLVM_INPROCESS=1` as the only arm difference: 25/26
byte-identical; 26/26 compiles carried the liveness line (the harness fails
any arm that compiled without announcing); the 1 DIFF is the same
pre-existing `perry-ext-http` panic family, delta = OS thread id only.

**Single-module end-to-end:** `spike.ts` output byte-identical across
text arm, in-process arm (both shim and integrated), and the pinned Node
oracle (26.5.1). A featureless build with `PERRY_LLVM_INPROCESS=1` fails
loudly with a rebuild hint rather than silently serving the text path
(verified), and the featureless build's normal path is unaffected
(verified).

**Textual round-trip (`.ll` → parse → print):** classes of difference, each
mechanical: explicit default alignment (`alloca i64` → `alloca i64, align 8`),
attribute-group renumbering (`#2` → `#0`), float formatting (`0.0` →
`0.000000e+00`; NaN-box hex doubles print identically), `ModuleID`/
`source_filename` headers, empty-metadata placement. Zero semantic diffs.
This is the normalization catalogue a Phase 2 construction-parity harness
must account for.

**`-fno-math-errno`:** the C API has no equivalent knob, so it was measured
instead of assumed: on macOS/arm64 the flag produces a byte-identical object
on Perry IR (Darwin already defaults to errno-free math). **Re-measure on
Linux before trusting the in-process backend there** — glibc targets default
the other way.

## Measurements (honest scope)

Environment caveats, stated up front: dev box under heavy load (loadavg
29–50 from concurrent builds — every number below is *indicative only*),
perry built at `perry-dev` profile, spike at `opt-level=1`. Nothing here is a
quotable benchmark; the quotable run needs a quiet box and the big corpus.

| Metric | Text path (clang subprocess) | In-process | Note |
|---|---|---|---|
| 245 KB module, `-O3` avg of 5 | Apple 21: 123 ms · Brew 22: 147 ms | 171 ms | subprocess spawn is **not** the bottleneck at this scale |
| 421 KB module (`batch.ts` kernel), avg of 3 | Apple 21: 241 ms | 434 ms | same conclusion; O3 dominates |
| Transient IR on disk | full module text per compile | **0 bytes** (memory buffer both directions) | eliminated by construction |
| Object bytes vs same-version clang | — | **byte-identical** | strongest possible parity |
| Error reporting | text positions in the temp file | verifier/parse errors from the live module | construction-site errors need Phase 2 |
| `perry` binary (perry-dev) | 53 MB | 224 MB with static LLVM 22 feature build | distribution cost if ever default |

The compile-time *win* thesis was **not** demonstrated at small scale — and
that is a real Phase 0 result: the payoffs that justified the experiment live
at the 1.4 GB-of-IR scale (parse + disk transit), in version control, and in
what the seam unlocks (per-function opt levels instead of #4880's
whole-module demotion, DWARF via `DIBuilder`, in-process RS4GC). Those are
Phase 2/3 measurements, gated on a quiet box, ≥25 GB free disk, and the gap
suite green under the flag.

## The seam finding (differs from the brief's assumption)

The brief's Phase 1 places the seam "between lowering and IR construction"
with two builder implementations. The codebase disagrees: perry-codegen is
~189k lines of *string-oriented* emission (`LlModule` holds functions as
rendered text lines), so an instruction-level builder interface is not a
seam, it is a rewrite of the whole emitter. The seam this codebase already
has — and documents as such — is `compile_ll_to_object(ll_text) -> Vec<u8>`.
Swapping the transport there delivers the operational wins now (no
subprocess, no version skew, no disk transit, pinned LLVM, exact-version
`opt`-less RS4GC scheduling for #7174) at ~250 lines, byte-identical-proven.
The construction-level seam remains the right *eventual* target — it is what
kills the string-building CPU/RSS at scale and enables `DIBuilder` — but it
should be motivated by a measured profile of string-build cost, taken through
this transport, not assumed.

## Constraints and open risks

- **`cl::opt` state is process-global** (`LLVMParseCommandLineOptions`).
  Fine today (one env-derived value per process, applied under `Once`), but a
  per-function-opt future must use per-pipeline mechanisms instead.
- **Parallelism**: one `Context` per rayon worker compile (LLVM contexts are
  not thread-safe; separate contexts on separate threads are). This mirrors
  the parallel `clang -c` model, so peak-RSS behavior needs measuring at
  scale — a subprocess returns its memory to the OS at exit, an in-process
  context returns it to the allocator.
- **bindings lag**: llvm-sys tracked LLVM 22 within the release cycle
  (221.0.1 published; inkwell 0.9 already has `llvm22-1`), but a future LLVM
  bump waits on both crates. The subprocess path has the inverse problem
  (whatever clang the user has). Pinning wins, but it is a pin *we* must
  maintain.
- **Windows / cross targets**: untested. `Target::initialize_all` registers
  everything, and `-target` handling mirrors the clang path, but COFF and the
  MSVC/MinGW distinction (see `probe_clang_default_triple`) need their own
  A/B before any claim.
- **`-fno-math-errno` on Linux**: measure before enabling there (see above).

## Recommendation, with its price

**Adopt the transport seam** (this branch's `llvm-inprocess` feature) as the
vehicle for the remaining phases; keep it opt-in until the gap suite has run
green under `PERRY_LLVM_INPROCESS=1` on macOS *and* Linux and the Claude
Code bundle has been compiled through it with the measurement table filled on
a quiet box. Price: LLVM 22 as a build-time dependency *for those who enable
it* (+171 MB on a perry-dev binary with static linking; dynamic linking
against `libLLVM.dylib` is the dev-loop alternative), llvm-sys/inkwell as
maintained pins, and the process-global constraints above. Do **not** start
the instruction-level builder until the string-build cost has been profiled
through this transport at bundle scale — if parse+build turns out to be a
minor fraction of `default<O3>` even at 1.4 GB, the rewrite's payoff is
DIBuilder and error locality only, and that should be decided with the number
on the table.

## Reproducing

```bash
# Spike (standalone, no Perry workspace impact)
cd experiments/llvm-inprocess-spike
LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm cargo build --release
./target/release/perry-llvmc-spike demo demo-out

# A/B through unmodified perry (shim as PERRY_LLVM_CLANG)
PERRY_LLVM_CLANG=$PWD/target/release/perry-llvmc-spike \
PERRY_LLVMC_SPIKE_LOG=/tmp/live.log \
  perry spike.ts -o spike_inproc

# Integrated backend (one binary, flag-gated A/B)
LLVM_SYS_221_PREFIX=/opt/homebrew/opt/llvm \
  cargo build --profile perry-dev -p perry -p perry-runtime-static \
  -p perry-stdlib-static --features perry/llvm-inprocess
PERRY_LLVM_INPROCESS=1 perry file.ts -o out   # stderr must show the liveness line

# Corpus A/B
experiments/llvm-inprocess-spike/batch_ab.sh /tmp/batch-ab
```
