### `a[i]++` / `o.f++`: the result is a precise root, and the operand half was not a bug (#7628)

The member read-modify-write arms consume their operands at four calls with
collection points between them:

```text
old     = js_dyn_index_get(obj, idx)   ; a getter / Proxy trap
old_num = js_to_numeric(old)           ; a valueOf
new     = js_numeric_step(old_num, s)  ; allocates only (#7198)
          js_dyn_index_set(obj, idx, new)   ; a setter / Proxy trap
```

#7628 filed the group-wide single re-read as a live #7154 and asked for a
per-use-re-read combinator. **Slice 6 had already built one** — `RootedGroup` is
one scope re-readable at any number of caller-chosen points — so both arms use
it and no new primitive arrives with this caller.

**The operand half turned out not to be a live bug, and the sabotage arm is how
that was established rather than argued.** Collapsing the per-use re-reads back
to one — and, for `PropertyUpdate`, removing the receiver's root outright —
leaves the emitted IR unchanged in the relevant respect: `root_reload` (#7280)
rematerialises the slot load at every use a collection point can reach,
*including* through the `ptrtoint` + `and POINTER_MASK` handle derivation that
#7280's own taxonomy lists as case (a), the class it cannot repair. That entry
is about a raw handle a helper *returns*, not one masked out of a NaN-boxed
value the pass has spilled. The per-use re-reads are kept because they cost
nothing and remove the dependence on a pass carrying a documented side
condition, but they are documented as belt-and-braces, and the two tests that
cover them are named as pipeline assertions rather than lowering assertions.

**The repair is the result.** For a BigInt element `js_to_numeric` /
`js_numeric_step` hand back a heap `BigIntHeader`, and whichever value the
expression yields — `old_num` for postfix, `new` for prefix — is live across the
write, i.e. across a user setter, as a bare call result with **no slot** for
`root_reload` to reload from. That is the taxonomy's case (d).
`RootedGroup::adopt_emitted` closes it, gated on `is_provably_not_bigint` so a
typed-array `ta[i]++` keeps the IR it had. The gate's counterfactual is measured
rather than assumed: the typed-array arm's returned register is produced *above*
the write, the same shape as the unrooted lowering.

`test-files/test_gap_7628_index_update_rooted.ts` pins the semantics the repair
must not perturb (both fixities, BigInt elements, a `valueOf` receiver, the
lodash `countBy` shape, once-only index evaluation) byte-for-byte against node
26.5.1. `expr/issue7628_rooting_tests.rs` carries the IR-ordering assertions and
records both sabotage arms.

The two arms moved to `crates/perry-codegen/src/expr/member_update.rs`;
`instance_misc1.rs` was 4 lines under the 2000-line cap and the change pushed it
over.
