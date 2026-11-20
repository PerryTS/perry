### `gc-root-dominance` now checks the lowering that actually ships

`scripts/gc_root_dominance_check.py` gained a **`--statepoints`** mode that
reads `gc.statepoint` `"gc-live"` relocation bundles, and
`scripts/gc_root_dominance_corpus.sh` gained **`--lowering native`** to emit a
corpus it can read. Both shadow-stack modes are unchanged and still gated; the
new arm runs in a separate, deliberately non-required job,
`gc-root-dominance-statepoints`.

Until now the corpus was compiled under `PERRY_RS4GC=0` — the shadow stack —
because the checker anchored on `@js_shadow_slot_bind` and the native lowering
emits zero of them. Since #7370 native roots are the default on every target
whose frames the runtime can walk, so **a green `gc-root-dominance` was
evidence about a lowering that does not ship**. `docs/src/internals/gc-rooting-invariant.md`
carried that as its first blind spot; it now documents both lowerings, and
each mode **refuses the other's corpus** rather than reporting it clean (the
native corpus has zero root stores, the shadow corpus zero safepoints).

**The corpus needed one extra step, and it is single-sourced.** `--trace llvm`
dumps what codegen emitted, and codegen does not emit statepoints — it emits
`ptr addrspace(1)` root allocas and a `gc "statepoint-example"` attribute, and
LLVM's `rewrite-statepoints-for-gc` inserts the safepoints later, in the linker
step. Measured: the traced IR under the default lowering contains **zero**
`gc.statepoint` instructions and zero `"gc-live"` bundles. So the native corpus
is the traced IR plus the production rewrite, run with the pass string read out
of `STATEPOINT_REWRITE_PASSES` in `perry-codegen/src/inprocess.rs` — the script
refuses to run if it cannot read that const, because a reproduction of a
pipeline that has drifted is a corpus about nothing.

**What it checks.** A value is a root at a safepoint iff it appears in that
safepoint's `"gc-live"` bundle, and its identity below is the `gc.relocate`
result — so "the root store must dominate every later collection point" becomes
*no register naming a GC object may be used below a safepoint unless it is the
relocated value*. The load-bearing distinction is `ptr addrspace(1)` (tracked:
LLVM relocates it and rewrites its dominated uses, so it is never stale) versus
everything else (invisible to RS4GC — and Perry NaN-boxes, so a JSValue spends
most of its life on the wrong side of that line). Hits split into `unrooted`
(nothing in the cast chain is in the bundle; the object is unmarked) and `stale`
(the object is relocated but a raw copy of its pre-move address is used below),
because those have different fixes. Two properties fall out that the shadow
modes cannot have: `NONCOLLECTING` is not consulted at all — LLVM already
decided which calls are safepoints and put the answer in the IR — and every
safepoint names its wrapped callee, so `--moving-only` classifies against the
real symbol.

**A pre-existing parser bug, found because a seeded violation went unreported.**
`DEFINE_RE` could not match a **quoted** function name, and LLVM quotes any
identifier containing `$` — i.e. every representation-selection specialisation
(`…$typed_f64`, `…$generic`, `…$spec_i32`). The `define` line did not match, the
parser's function cursor stayed unset, and the whole function body was skipped
in silence: **175 of 2452 defines (7.1%) in the native corpus**, and not a
random 7% — repsel specialisations are exactly where a representation change
moves the rooting obligation. Zero in the shadow corpus (perry's writer prints
those names bare), so the existing arms were never affected. A `define` the
parser cannot name now raises instead of being skipped. The same round fixed
three more shapes LLVM's printer produces and perry's writer never does:
`invoke` destinations on a continuation line (349 in a 21-module corpus — read
line-at-a-time, every block below an `invoke` was unreachable and dominance
answered False for all of it), `landingpad` clauses on another, hyphenated
`split-lp` labels, and LLVM's `; (%orig, %base)` relocation annotations being
read as uses of `%orig` (186 of 326 hits on the first measured corpus).

**Proof it can fail.** `--seeded-violations 40` splices a safepoint between a
real `ptrtoint ptr addrspace(1)` and its real use in the emitted IR and requires
all 40 to be reported (40 planted, 40 caught, 0 missed); it now runs
unconditionally rather than after an early return on real violations, and is a
usage error on the two modes that have no seeder rather than being silently
ignored. `--self-test` gained the statepoint half, sabotage-tested arm by arm —
ten sabotages, each turning it red and naming its own arm. Two of those arms
were vacuous when first written: the tracked/untracked line had no fixture at
all (a cast-closure that walked through it reported the *relocated* value as
stale — 21 of the first 29 hits, all correct code), and the `--min-statepoints`
floor was passing via `--min-relocates`. A structural assertion — every
`"gc-live"` operand must be recognisable as `ptr addrspace(1)` — caught 262
unreadable operands on first run, all relocation phis, because
`addrspace\(1\)\b` has no word boundary between `)` and a space.

**What it found.** Over the curated corpus (149 modules, 2452 functions, 30033
safepoints, 17478 with a live bundle, 40759 relocations): **21 hazards under
`--moving-only`, all `unrooted`**; 1444 unfiltered. They are known shapes — nine
string-literal handle globals held across a call (#7240's shape, whose fix
covers the shadow lowering only), seven unmasked receivers dereferenced below a
property GET (#7280 / zod's `clone`), two module-global reads, a
`js_new_target_get` → `js_new_target_set` pair structurally identical to #7226's
`prev_this`, and one closure-capture read. They are **not** allowlisted: the
gate carries `--max-unrooted 21`, a ratchet that can only be lowered, with
`--max-stale 0` holding the other class at zero. The 21 are enumerated by shape
in #7664, which is the budget's referent; each shape wants its own fix and its
own decrement before the job is promoted to a required context.

The #7210 `IMMOVABLE_SOURCES` exemptions carry over rather than being
re-derived — that adjudication is about the allocator, not the lowering, and
re-deriving it is the mistake that made #7240 invisible. Wiring them through
removed 187 of 326 hits on a probe corpus.
