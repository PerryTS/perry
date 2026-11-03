### repsel: object-literal element types reach the element-shape loop clone (#7480)

`for (let j = 0; j < n; j++) sum += keep[j].v` over a **`keep: {v, w}[]`** —
#7480's own kernel — went from **408 ms to 12 ms** on the pinned quiet mini
(200k elements × 50 sweeps, 7 interleaved rounds, checksums equal in every
cell). That is **parity with node (12 ms) and bun (12 ms)**, down from 34×
node. The named-class arm the #7612 consumer already covered is unchanged at
13 ms, and the region-local arm `collectors/ptr_shape_elements.rs` (#7034 §3)
already covered is unchanged at 14–15 ms.

**Root cause.** `stmt/element_shape_loop.rs::element_class_name` resolved
`Array(Named(C))` only, so a declared object-literal element type never
produced a class id and #7480's kernel never reached the clone. It now also
resolves the declared object type to the `__AnonShape_<hash>` class its
literals allocate, by matching the declared property order against the module's
anon shapes. The name cannot be recomputed — `mint_anon_shape_class` keys its
FNV hash on the literal's *inferred value* types (`{v: 1}` tags `i`, not `n`)
while the annotation says `number` — so the class is looked up, not derived,
and an ambiguous field-name list **declines** rather than guessing (`ctx.classes`
is a `HashMap`; "first match wins" would make the emitted code depend on
iteration order).

**`receiver_class_name` is deliberately unchanged.** Widening it to type an
`Object`-typed element read is the #6377 blast radius #7612 refused. The clone
was made self-contained instead: its `ElementShapeLoopFact` already carried the
class name and packed slot index, so the three sites that would otherwise
re-derive the class from the receiver now consult one predicate,
`expr::element_shape_loop_fact_for_property_get` — the raw-f64 field lowering
(its interception moved *above* the `receiver_class_name` gate),
`type_analysis::is_numeric_expr`'s `PropertyGet` arm, and `expr::binary`'s
arithmetic-operand router. All three are scoped to the fast clone, so
`keep[j].v` anywhere else is byte-for-byte what it was. The predicate includes
the canonical-i32 counter slot, because answering `yes` is a promise that the
read takes the bare-load lowering and `is_numeric_expr` bets a raw `double` on
that promise.

**The issue's cost model was wrong, and the correction is the design point.**
#7480 recorded "no out-of-line guard *calls*, the cost is stacked inline
diamonds"; the object-literal arm actually carried three calls per iteration,
the third being `js_dynamic_string_or_number_add` — with no resolvable class
the accumulator also loses its numeric proof, so `+` is not an `fadd`. That was
recorded as "a second, separable lever". It is not one: the clone is admitted
only if it is provably call-free (`LlBlock::contains_gc_unsafe_call` counts
every non-`llvm.` call), so resolving the element class *without* restoring the
numeric proof emits the clone, fails the call-free test, branches
unconditionally to the slow arm and buys zero at a cost in code size. Anything
gated on call-freeness has this shape. The numeric claim inside the clone is
also stronger than the annotation it replaces: the residual per-element check
already proves `GC_OBJ_TYPED_LAYOUT_INTACT`, i.e. that the slot holds a raw
double.

**An existing gate that could not fail.** `fast_clone_slice` in
`element_shape_loop_tests.rs` sliced from the first *substring* match of
`for.element_shape_fast.cond`, which is the `br label %…` terminator of the
fast preheader — four lines above the slow preheader — and every assertion made
against the result is a negative (`!fast.contains(" call ")`, …). The IR census
that exists to prove the clone is call-free had therefore been **vacuous since
#7612**, on the code that then shipped the #7660 SIGBUS. It now finds the block
*definition* and asserts the slice contains the cloned body and its element
load, so it cannot pass on an empty subject again (#7024/#7025 family).

**Coverage.** Seven new IR-census tests: one positive that asserts the clone is
reached, call-free, and `fadd`-accumulating (all three together, because any
one alone is inert), plus sabotage cases for an ambiguous shape, a tie a field
type *can* break, an optional property, a shape no literal allocates, a
reordered shape, and a read outside the clone that must stay on the by-name
path. `test-files/test_gap_repsel_element_shape_loop_clone.ts` gains an
object-literal section covering the #7660 growth-forwarding shapes on this arm
(callee-built, callee-filled, a 17-element prefix, and a module-global array
read from inside a function), an inline `rows.length` bound the matcher rejects,
a layout downgrade that forces the residual check's side exit mid-loop, two
anon shapes sharing field names, and a mixed numeric/string shape the matcher
must decline — byte-identical to the Node 26.5.1 oracle.

`docs/engine-plan.md` item 6 is closed. What remains of Route A — a `Ptr<Shape>`
element representation that survives *outside* a loop for the parameter/global
case — needs the type-visibility change above with its own gap-suite A/B, and
is scoped against the region-local case already being covered.
