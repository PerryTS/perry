### fix(runtime): dispatch inherited Array statics on a subclass constructor (#7541)

`[...MyArr.from([1, 2, 3])]` (where `class MyArr extends Array {}`) threw
`TypeError: value is not iterable`. The spread was never at fault — a
directly-constructed subclass instance has always spread correctly. **`MyArr.from`
resolved to nothing**: `Array.from` / `Array.of` / `Array.isArray` are folded in
the HIR on the *literal identifier* `Array`, so a subclass receiver matched no
fold, fell through to `js_class_static_method_call`, and hit its documented
miss-fallback — which returns the RECEIVER. The call therefore evaluated to the
class ref, which is genuinely not iterable. Same "keyed on a literal `extends`
name" weak area as the instance-side native-base gaps, on the static side.

`js_class_static_method_call` now has an Array arm beside the existing
`class X extends Promise` and `class X extends Buffer` ones, gated on the same
bounded class-chain walk (`is_array_subclass_class_id`). Both spec statics were
already implemented constructor-aware — `array_from_full` / `array_of_full` run
`Construct(C, …)` when `IsConstructor(this)`, and a class ref answers true — so
passing the subclass receiver through builds a real subclass instance, matching
`Array.from.call(MyArr, x)` in node rather than degrading to a plain `Array`.

Depends on #7574's funnel: `array_from_full` installs elements through
`CreateDataPropertyOrThrow`, which branches on `js_array_is_array` — true for a
subclass instance — and so reaches `js_array_set_f64_extend` on what is
physically an `ObjectHeader`. Without that fix this change would construct the
right object and then write into its header.

Validated by `test-files/test_gap_7541_array_subclass_inherited_statics.ts`
(byte-identical to node, exit 0; covers `from` with a mapFn / from a `Set` /
from an array-like, `of`, `isArray`, an indirect subclass, and the full
iteration surface on a static-produced instance), plus a full runtime revert
reproducing the reported `TypeError` verbatim.

Still open, deliberately: the property-GET form (`typeof MyArr.from` reports
`undefined` — only the call form is dispatched, as is already the case for the
Promise/Buffer subclass statics), `sub instanceof MyArr` (the class-registry
parent-edge gap, Array sibling of #7575), and `ArraySpeciesCreate` on
`sub.map(f)`.
