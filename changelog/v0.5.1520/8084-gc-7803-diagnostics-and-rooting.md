### GC: five diagnostics, two rooting fixes, and the corpus/lowering cell nobody gated (#7803)

`#7803` — the `zod` dependency corpus dying under a seeded GC schedule — is now
localized but **not fixed**. It fails at `zod/src/v4/core/parse.ts:65`
(`result.issues`, where `schema._zod.run({ value, issues: [] }, ctx)` returned
`undefined`), all three observed messages are one loss seen at different points,
and the failure needs the `new Function` path: `jitless` gives 0/16 against
8/16 with it. Five hypotheses were tested and refuted or left unsupported; the
audit trail, including the null results, is in `gc-handoff/ZOD-NOTES.md`.

**Two rooting defects found and fixed on the way:**

- **The callee did not outlive the arguments** in three call-lowering arms
  (`expr/new_dynamic.rs` ×2, `expr/call_spread.rs`,
  `lower_call/early_branches.rs`). Each lowered the callee into a bare
  register, lowered the arguments after it — every one of which can allocate —
  then handed the consuming call the original register. Under the shipping
  statepoint lowering that register is in no live bundle, so nothing marks it
  and nothing relocates it. Fixed with `rooting::RootedGroup`; a root rather
  than a reload, because JS resolves the callee *before* the arguments and
  re-reading below them would pass whatever an argument assigned.
- **A stale argument buffer** in two dispatch arms of `js_native_call_method`,
  both with a verified collection point between the handle scope and the
  dispatch. `arg_handles` is what the collector rewrites; the caller's
  `args_ptr` is not.

Measured on the dependency corpus under the shipping lowering: **66 → 3**
unrooted hazards (`js_new_function_construct` 24→0,
`js_closure_call_apply_with_spread` 16→0, `js_closure_call1/2` 23→0).

**`gc-root-dominance.yml` emitted three of four corpus × lowering
combinations.** #7280 fixed the population (curated files lack the shapes real
libraries produce) and #7452 fixed the lowering (statepoints ship; a shadow
corpus contains none of that root form) — neither reached the other's cell, so
the `zod` corpus compiled the way shipped binaries are compiled had never been
checked. It read 66 where the curated arm is calibrated to zero. Now emitted by
`scripts/gc_root_dominance_dep_native_corpus.sh` and gated at
`--max-unrooted 3 --max-stale 0`, a budget that can only go down.

**The `dyn_eval` interpreter was untestable, not merely unrooted.** It offered
the collector no cooperative safepoints, so `PERRY_GC_ZEAL` and
`PERRY_GC_SCHEDULE_SEED` ran straight past it while the static checker had no
IR to read — leaving `dyn_eval/mod.rs`'s claim that interpreter frames hold
*every* live JSValue in a rooted stack unfalsifiable by anything in the tree.
`PERRY_GC_INTERP_SAFEPOINTS=1` closes that: `loop_polls` 24,029 → 93,210 on one
binary, i.e. the interpreter was ~74% of that workload's potential safepoints.

**New diagnostics**, all default-off and all parsed by value rather than by
presence (the `PERRY_GC_DIAG=0`-enables-diagnostics footgun of #7993 does not
get repeated):

| knob | what it does |
|---|---|
| `PERRY_UNCAUGHT_BACKTRACE` | symbolicated native backtrace on the uncaught-throw path |
| `PERRY_KEEP_SYMBOLS` | skip only the final `strip`, leaving `-g` off — `PERRY_DEBUG_SYMBOLS` does both, and `--debug-symbols` SUPPRESSES #7803 (0/13 against 44%), so an instrument that needs symbols must not use it |
| `PERRY_GC_INTERP_SAFEPOINTS` | cooperative GC safepoints at every `eval_expr` / `exec_stmt` |
| `PERRY_GC_POISON_FROMSPACE` | poison retired from-space in place, changing no layout |
| `PERRY_GC_TENURING_SURVIVALS` | pin the promotion age past the adaptive threshold |

The pin-latch abort now also prints **which copying-minor walk** handed it
the header (`copying walk phase: <scanner | remembered_set | worklist_drain
| mutable_root_slots/{shadow,native,global}>`) and a mutator backtrace.
On this corpus the latch fires on an *incoherent* header (INTERNED on a
Map, a 2 GiB nursery size) — a stale slot, not a real pin. Seed 3 under
`RATE=0.1 ALLOC_KB=0` is a 3/3 abort; the slot is a native stack-map
root in `Doc.write` / `generateFastpass` / `$ZodObjectJIT.parse`.
No new knob.

**ROOT CAUSE, FOUND AND FIXED: the spread-`new` bundle wrote through a moved
accumulator.** `Expr::NewDynamicSpread` (`new F(...args, src)`) folded its
arguments into a single array with the accumulator in a bare i64 register.
Every regular argument's lowering and every spread part's
`js_array_like_to_array` can run a moving minor; the following
`js_array_push_f64`/`js_array_concat` then wrote a NaN-boxed element through
the accumulator's pre-move address — into from-space pages the same cycle had
already recycled into Eden, over whatever live young object occupied them.
The element is typically a string, and every garbage header the pin-latch
ever recorded on this bug is the **high half of a NaN-boxed string** (sizes
`0x7FFF02AB/0x7FFF03AF/0x7FFF03FF/0x7FFF02E8/0x7FFF0438`, their low bits
tracking each run's ASLR heap base). zod's `Doc.compile` — `new F(...args,
lines.join("\n"))`, the closing expression of every `generateFastpass` — is
the corridor that hit it: that is why the failure needed the `new Function`
path (jitless 0/16), why `parse.ts:65` read `.issues` off garbage, and why
the victim frame varied (the latch names whoever points into the sprayed
neighborhood; the frame-namer diagnostic placed it in `$ZodObjectJIT.parse`'s
bundle at its `fastpass(payload, ctx)` statepoint, SP+40). The callee had the
same defect — this arm was not among the three `8842a0be4` fixed.

Both `NewDynamicSpread` and the dynamic `super.m(...spread)` arm (an
identical private copy of the loop) now route through
`call_spread::bundle_args_rooted` (`pub(crate)` so private copies cannot
exist), with the callee in a `RootedGroup` re-read below the bundle.
Regression tests assert the IR ordering that IS the fix — the accumulator
each fold reads and the callee the dispatch reads are defined below the
bundle's last collection point — and were verified to fail against the
pre-fix lowering. The pin-latch abort now also names the owning frame,
statepoint register/offset and slot address of the stale native root
(`native root slot: owner=…`), dladdr-symbolicated.

**SECOND ROOT CAUSE, FOUND AND FIXED: the compact GC map collapsed RS4GC
(base, derived) pairs — gc_map v4.** The compact stack-map format was built
on the stated premise that "Perry has no interior pointers" and folded every
statepoint (base, derived) pair into one slot. The premise is false: the
RS4GC prelude (`mem2reg,sccp`) hoists for-of element GEPs into values that
live across polls, which LLVM records as DERIVED pointers. With the pairing
discarded, the runtime walker treated `&elements[i]` as an object start —
misreading array element words as a GcHeader (the pin-latch's
INTERNED-on-map / `0x7FFF…`-size aborts were exactly that) — and never
rewrote the cursor as `base' + delta` when the array moved, dangling it.
Format v4 keeps `(base_index, reg, offset)` derived entries (repeat-flag
shared, version-gated fail-closed on both sides), and all three walkers
(Itanium unwind, aarch64 fp-chain, Windows RtlVirtualUnwind) exclude derived
slots from the visited-root set and rewrite them from their base after it
moves, preserving the slot's stored form. Measured on the pinned zod
schedule: seeds 1, 2 and 5 flip from ~2/3 aborting to 0 failures (with
`copying_minors>0` asserted per run); seed 3 retains one residual window,
characterized to the exact slot, record and creation cycle in
`gc-handoff/ZOD-NOTES.md` §40.

Also landed: the remembered-set rebuild for promoted objects now runs AFTER
the worklist drain (it previously covered only root-phase promotions —
drain-phase promotions, i.e. everything transitively reachable, were
appended to `moved_headers` after the rebuild had already run); the
spread-`new` and dynamic `super.m(...spread)` bundles route through the
rooted accumulator with the callee in a `RootedGroup` (IR-ordering tests,
verified to fail against the pre-fix lowering); the whole-heap from-space
scan stops at an array's length (unused capacity on a hole-reused block
holds the previous occupant's bytes and manufactured a deterministic false
MISSING-REWRITE); and four new #7803 instruments — the pin-latch names the
owning frame, register, offset, slot address and the census-backed enclosing
object of its target; `PERRY_GC_THIS_SET_CHECK` traps incoherent values at
the implicit-this boundary in both directions; `PERRY_GC_NATIVE_SLOT_VERIFY`
aborts on the cycle that CREATES a stale native slot with the rewrite walk's
stats and the collector's own classification of the target.
