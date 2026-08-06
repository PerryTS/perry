**A closed-shape object literal built in a `for` loop is no longer declared to
the collector as N pointer slots (#7544).**

#7532 left a note saying `mint_anon_shape_class` "gives every synthesized field
type `Any`". It does not — it writes each property's inferred type through
verbatim, and `{ v: 1, w: 2 }` already minted two `Number` fields, already got a
raw-f64 mask with a null pointer mask, and already took #7532's allocation-site
declaration.

What was `Any` is the **loop** form, and the cause is one hardcoded type.
`for (let i = 0; …)` wrote `Type::Any` for its head binding while the
statement-level `let i = 0;` runs `infer_decl_type` and gets `Number`. So
`infer_type_from_expr` saw `i: Any`, `i + 1` inherited it, and
`{ v: i, w: i + 1 }` minted two `Any` fields. `Any` is pointer-bearing, so the
most common allocation shape in the language told the GC it held two references
when it provably held two doubles — and `class_layout_declarable_at_allocation`
correctly refused, so it also missed #7532's declaration entirely.

Both `for`-head declarator sites now call the same `infer_decl_type` the
statement-level declarator uses. `var` heads keep `Type::Any`: a `var` head
binding is function-scoped and var-hoisted, so its declarator is not the only
writer of the name and the statement-level parity argument does not carry over.

**What is propagated is the initializer-inferred type of the head binding** —
byte-for-byte the computation `let i = 0;` has always performed. No new *kind*
of fact enters the layout; the annotation channel (`for (let i: number = 0; …)`)
is the one `let y: number = 0;` already carried into anon-shape fields. A bare
parameter, an explicit `any`, a `var` head, and an inner shadow all still mint
`Any`.

This is **not** a runtime type assertion. Perry validates no declared type, so a
`Number`-typed field can still receive a pointer through a later dynamic write.
That is discharged where #7532 discharged it: the raw-f64 store guard rejects
the non-double bits, falls back to the boxed setter, and `layout_note_slot`
downgrades the descriptor to `GC_LAYOUT_UNKNOWN`.

IR census on `{ v: i, w: i + 1 }` in a loop — identical source, only the
compiler differs:

| call | before | after |
|---|--:|--:|
| `js_gc_init_typed_shape_layout` (post-constructor) | 1 | 0 |
| `js_gc_declare_typed_shape_layout` (at allocation) | 0 | 1 |
| `js_gc_note_slot_layout` | 3 | 1 |
| `js_write_barrier_slot` | 3 | 1 |
| `js_string_addref_if_heap_string` | 3 | 1 |
| `js_dynamic_string_or_number_add` | 1 | 0 |

and the mask flips from `pointer_mask = [i64 3]` to `raw_f64_mask = [i64 3]`
with a null pointer mask — a pointer-free layout.

**Two corrections to the expected evidence, recorded because both would have
been wrong claims.** The anon-shape constructor's stores never routed through
`js_put_value_set` — that was #7532's *declared-class* path; the synthesized
ctor's stores are direct-GEP, and what they shed here is the three-call
bookkeeping preamble. And the collector's **byte** counters do not move:
`copied_bytes 361760`, `promoted_bytes 91216`, 8 cycles, byte-identical across
both arms. On reflection that is expected — `mark_field_into_worklist`
re-validates every slot word and rejects a double, so declaring the slots
pointers cost a visit and a reject, never retention. The win is scan work and
store bookkeeping, not retained bytes.

New tests. `crates/perry-hir/tests/anon_shape_field_types.rs` pins both
directions of the boundary and is sabotage-tested: stubbing
`for_init_binding_type` back to `Type::Any` turns 4 of its 7 tests red and
leaves the 3 boundary cases green. `test-files/test_gap_7544_anon_shape_numeric_fields.ts`
is the runtime witness, byte-diffed against the node oracle — mixed literals
with a heap-string child retained across forced collections, pure numeric
literals, and objects whose *numeric* fields are overwritten with freshly
allocated heap strings and read back after two more collections. Its subject is
verified live in the emitted IR (three `js_gc_declare_typed_shape_layout` calls,
raw-f64 masks on the pure shapes, a pointer mask only on the mixed one), and it
runs clean under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` with `PERRY_GC_MOVING_LOOP_POLLS=1` at
compile and run time — 100 131 copying minors, 100 131 quarantined from-space
page-sets, exit 0 — and under `PERRY_GC_VERIFY_EVACUATION=1`.
