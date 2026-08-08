### Layer 1 rooting migration, slice 7 — the `expr/` operand-list modules (#7615)

`expr/child_proc.rs`, `expr/proxy_reflect.rs` and `expr/fs_await.rs` are migrated
onto `crate::rooting` and listed in the `MIGRATED_MODULES` ledger. All three
named `expr::temp_root` before the migration
(`temp_root_{push,get}_double`, `temp_root_truncate`,
`guard_store_operand{,_across}`, `reread_store_operand`,
`release_store_operand`, `expr_may_trigger_gc`), so all three lines are
load-bearing on the committed source and not only under sabotage.

**One node-visible bug, A/B'd byte-for-byte, and it is 14 dropped side effects.**
`child_process` validated each argument the instant it was lowered — lower
`command`, throw `ERR_INVALID_ARG_TYPE`, never evaluate `options`. JS evaluates a
call's whole argument list *before* the callee is entered, so node runs those
side effects and only then throws. Against node 26.5.1 with a baseline compiler
built from `main` in a separate target directory, a probe covering all seven
entry points (`execSync`, `spawnSync`, `spawn`, `fork`, `exec`, `execFile`,
`execFileSync`) diffs **empty** against the fixed arm and **14 missing lines**
against the baseline — every `args`, `options` and `callback` expression, never
evaluated. The relative order of the validators among themselves is unchanged
(`file`, then `args`, then `options`), which is node's own order in
`normalizeSpawnArguments`.

**The unprotected windows, in three families.** Nine across five
`child_process` arms, twenty-eight `Proxy.*` / `Reflect.*` lowerings, and one
accumulator shared by four call sites.

*Raw pointers held across user code — #7280 taxonomy (a), which `root_reload`
structurally cannot repair, because the value has already left the NaN-boxed
representation a slot can be re-read into:*

* `execSync` / `spawnSync` / `spawn` / `execFile` / `execFileSync` stripped
  `command`/`file` to a bare `StringHeader*` and then lowered `args` and
  `options` — arbitrary user code — before `js_child_process_*` dereferenced it;
* `fork` is #7453's shape at a second site: `js_jsvalue_to_string_coerce` (which
  runs a user `toString` on an object module path) produced a raw string pointer
  that was then held across two more lowerings;
* `spawnBackground` re-tagged `log_file`'s stripped pointer and carried it across
  `env_json`'s lowering;
* **`process.env[k] = v`** was the worst of them. The computed-key branch coerced
  the key with `js_to_property_key` — a **fresh** heap string with no other root
  at all — stripped it to a raw pointer, and only then lowered the value;
  `js_setenv` dereferenced whatever was left. The literal-key branch is #7114
  one operand over from the `PutValueSet` key #7201 fixed: the key's
  `__perry_init_strings_*` handle global is a registered root that evacuation
  *rewrites*, and the load sat above the window. ES2022 moved `ToPropertyKey`
  before the RHS, so the coercion cannot simply be sunk below the value — the
  coerced key is what has to survive.

*Operand-to-operand — taxonomy (c):* twenty-eight `Proxy.*` / `Reflect.*` arms.
`Reflect.has(target, key)` lowered `target`, lowered `key`, and handed the
pre-collection `target` register to `js_reflect_has`; `Reflect.set` does it with
four operands. Exactly one arm in that file made a rooting decision before this
slice (the `PutValueSet` write-IC), which is what "listed ≠ audited" looks like
from the other side — nobody had read the module.

*Accumulators — #7154:* `expr::helpers::proxy_build_args_array` threaded the
argument array's raw `*mut ArrayHeader` through its push loop in a bare SSA
register while each element was lowered, and had no way to root its **caller's**
receiver across the same loop. It is deleted; its four call sites now build the
array inside a `RootedGroup` that holds the receiver too.

**`fs_await.rs`'s root was correct and never released.** `temp_root_push_double`
ran unconditionally at the top of the `Await` arm and no path emitted a truncate.
Over-retention on every path in the alloca lowering, and in the FFI fallback
(`temp_pool_acquire` returns `None` when the function has no shadow frame) a
`js_gc_temp_root_push` per **execution** with no matching truncate — #7462's
unbounded-growth shape for an `await` inside a loop. The scope is now a
`RootedGroup` whose release `with_rooted_group` owns, emitted in the merge block.
The test asserts the consequence rather than the emission: three sequential
awaits must reserve the same number of rooted slots as one.

**The one API addition, and why it arrived now rather than earlier.**
`rooting.rs` deleted a `root_i64` combinator unused and wrote down the terms on
which a replacement could return — "with its caller and with a written argument
for why `call_rooted` cannot serve". This slice found two callers and they are
the same shape: a GC-managed value produced by an **emitted step** rather than by
lowering an `Expr` (the coerced `process.env` key; the assimilated promise the
await loop polls). `RootedGroup::adopt_emitted` is that combinator.
`call_rooted` cannot serve because it fuses the root store to a call it emits
itself with `Repr::Ptr` hardcoded, and neither value is the direct `i64` result
of one call. There is no protection *flag*: with no `Expr` to ask,
`Reload` cannot re-derive the value (re-emitting the producing call would call it
twice, and both producers are observable) and `Reuse` is the bug, so the answer
is always `Root` and a caller cannot pick the wrong one. What it weakens is
stated in its doc: `value` is a caller-produced register, so #7192's ordering is
writable here exactly as it already is in `with_rooted_accumulator`.

**Two judgement calls, recorded because they are decisions rather than
mechanics.**

* `child_proc.rs` re-reads its operands **twice** — once for the validators,
  once for the strip and the consuming call. `js_child_process_validate_options`
  reads a dozen own properties off a user object, allocating a key string per
  read; rather than depend on that being a non-collecting window (#7198's
  position), the second re-read removes the question. It is free where nothing is
  protected — `RootedGroup::reread` on an unprotected operand emits no IR — and
  one `load` where something is.
* `spawnBackground`'s `args` slot is lowered into the group with `collects =
  false`. Its value is evaluated for its side effects and then discarded
  (`js_child_process_spawn_background` takes no argument vector), and a value
  with no consumer has no window, so it is protected not at all.

**Cost, measured.** Over the `gc_root_dominance_corpus.sh` corpus (149 modules)
the root-store count moves **9799 → 9805**, +6, with violations 0 in both arms
and `--unrooted-allocas --moving-only` at 0 over 7864 gc-capable allocas. Three
zero-cost pins are committed so a future "root everything" change goes red: a
no-options `execSync`, a single-operand `Reflect.ownKeys` and a string-literal
`process.env` key must each emit no temp-root traffic at all.

**Tests.** `expr/slice7_rooting_tests.rs`, 13 cases, asserting IR *ordering*
rather than slot counts — a count lets one operand's rooting pay for another's
assertion — with a by-callee-name liveness assertion first in every one, so a
shape measured over a lowering that never ran cannot pass. Sabotage-verified
against the pre-fix source of all five touched files restored from `HEAD`:
`error[` count 0 (so the pre-fix source compiled and the run means something),
`Running unittests` present, **11 of 13 red**. The two that stay green are the
deliberate zero-cost pins, which are correct in both arms. The ledger sabotage
arm was run once per newly listed module — a compiling
`temp_root_push_double` / `temp_root_truncate` pair planted in each, `error[`
count 0 in all three, ledger test red and naming both planted lines.

**No runtime fault is claimed.** Every window here is demonstrated in IR. Whether
a stale pointer is *observably* wrong depends on what is recycled into those
bytes; the IR ordering is the evidence the window exists, and a fault would only
have been evidence that one arrangement reaches it.
