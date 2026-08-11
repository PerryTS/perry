### Layer 1 slice 1 — `lower_array_method.rs` migrated onto the rooting API (#7615)

`crates/perry-codegen/src/lower_array_method.rs` is now migrated end to end onto
`crate::rooting`, following the template #7617 established, and is listed in
`rooting::MIGRATED_MODULES`. It is the campaign map's highest hazard density: 37
raw sites, 40 hazard sites, and **zero** rooting references before this.

**The hazard was structural.** `lower_array_method` lowered the receiver *above*
the `match`, so every one of its ~30 arms held a NaN-boxed array pointer in an
SSA register across the lowering of its own arguments. Any argument that runs
user code — a callback literal (`js_closure_new` allocates), a call, an array
literal — is #7453's window, in `arr.map(cb)`, `arr.filter(cb)`, `arr.sort(cmp)`,
`arr.concat(f())`, `arr.splice(i, n, mk())`.

**What is new, stated against #7280.** `root_reload.rs` already re-read
shadow-slotted receivers below collection points, so the common case was not
stale. Three things it structurally cannot cover, and this migration closes:

- **A receiver reassigned by its own argument.** #7280 deliberately leaves the
  register alone there, because re-loading would observe the assignment
  (`operand_is_reloadable`'s documented miscompile). The register is then stale.
  A temp root is the one strategy giving both the call-time value and a
  rewritten address. Verified in emitted IR for `a.concat(reassign())` and
  `a.indexOf(reassign())` where `reassign()` allocates and reassigns `a`.
- **Argument-to-argument windows** — `arr.splice(mk(), mk(), mk(), mk())` held
  each evaluated argument across the next one's lowering, with no slot to reload.
- **A dead duplicate unbox in the `sort` comparator path**, referenced exactly
  once (its own definition). Removed; the only instruction this change deletes.

**How.** One `rooting::with_operands_rooted` around the whole `match`, over the
receiver plus the arguments the arm consumes; those are declared once in
`lowered_arg_count`, and the arms only emit. `lower_expr` no longer appears in
the file, which makes "no operand register crosses a collection point" a
property of the module rather than of each arm's author. Under-counting the
table is loud (a codegen panic on the first program that reaches the arm);
over-counting is benign (JS evaluates every argument anyway). Counts match
pre-migration behaviour exactly, so nothing changes semantically.

**IR identity.** Over 7 probes reaching all 46 of the module's distinctive
callees: 90 of 95 functions identical, and all 5 differing ones are the probe's
`main`. Whole-corpus net delta is root plumbing only — 0 non-plumbing additions,
one non-plumbing removal (the dead `sort` unbox). The −36 `load double` are
#7280's reloads, replaced by root reads. All 7 probes produce identical stdout
and exit codes on both arms, and the new arm is unchanged again under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`.

**Verification** (local; the CI backlog is deep). `gc-root-dominance` both gated
modes: 129/129 sources, 149 modules, 2452 functions, 9810 root stores → 0
violations, `--seeded-violations 40` → 40/40 caught, `--unrooted-allocas` → 0.
The baseline arm reads 9803 root stores over the same corpus, so the gate's
subject is demonstrably live rather than absent. All four checker static audits.
`cargo test -p perry-codegen --lib` (691) and `--doc` (both `compile_fail,E0499`
arms still reject); `cargo test -p perry-runtime --no-fail-fast` (1886).
`./run_parity_tests.sh --filter test_gap_array` against node 26.5.1 on **both**
arms: 13/13, identical (empty) failure sets. Ledger sabotage: a real
`temp_root_*` pair reintroduced into the migrated module compiles, and the
ledger test goes red naming both lines — the same answer #7617 measured, now
confirmed for a second module.

**Deliberately not closed here:** the `toString` arm's `unbox_str_handle` window
(#7213). Closing it needs a combinator that roots across a collection point the
module *emits* rather than one it infers from an operand list; per the template's
rule, that should arrive with the slice that needs it, not ahead of one. The
reasoning is recorded in the module header.
