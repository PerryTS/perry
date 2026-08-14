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
