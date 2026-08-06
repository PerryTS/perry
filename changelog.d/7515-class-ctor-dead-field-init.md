### Codegen: the dead-default-field-init elision now reaches user-written constructors (#7512)

#7469 elides the default-`undefined` write for a class field the constructor's
own prologue provably overwrites. Its changelog said it covered "plain user
ctors like `constructor(a, b) { this.a = a; this.b = b }`". It did not, and
could not: `ctor_prologue_param_assigned_fields` matched the prologue on
`Expr::PropertySet`, and **no user syntax lowers to that node**.
`perry-hir/src/lower/lower_expr/assignment.rs` turns every source-level
`obj.prop = value` — `this.v = v` included — into the spec `PutValue` node
`Expr::PutValueSet`. `Expr::PropertySet` is emitted only by *synthesized* HIR,
which is exactly what the anon-shape object-literal constructor
(`lower/context.rs::mint_anon_shape_class`) is built from. The elision was
therefore measured on the one construction form it reached, and was
structurally unreachable for the declared class it was documented as covering.

The visible consequence is the anomaly filed as #7512: `new Node(v, w)` with two
declared `number` fields was **slower** than the equivalent `{v, w}` literal, so
the most statically-known construction form in the language was the least
optimized one. Emitted-IR census of the two constructors, same workload, same
compiler (`--trace llvm`):

| per construction | `{v, w}` literal | `new Node(v, w)` before | after |
|---|--:|--:|--:|
| field-store IC diamonds | 2 | **4** | 2 |
| `js_typed_feedback_class_field_set_guard` | 2 | 2 | 0 |
| `js_class_field_set_fallback` | 2 | 2 | 0 |
| `js_array_numeric_value_to_raw_f64` | 0 | 4 | 2 |
| constructor body, IR lines | 111 | **314** | 157 |

The two extra diamonds each stored a compile-time-constant `undefined` that the
next two statements overwrote. Both took the cold by-name arm on *every*
construction rather than occasionally: a freshly allocated instance carries no
typed-shape descriptor (`js_gc_init_typed_shape_layout` runs after the
constructor returns), so a `requires_raw_f64` set-guard on a declared `number`
field cannot pass, and `js_class_field_set_fallback` — a feedback-fallback
record plus a linear-key-search `js_object_set_field_by_name` — ran twice per
object. A class whose fields are declared `any` paid the same two dead diamonds.

The fix is one recognizer, `prologue_assigned_field`, accepting both spellings
of `this.<field> = <plain parameter>`. The proof obligation is unchanged and is
about the *operands*, not the store opcode: `This` and `LocalGet` of a plain
parameter cannot throw, allocate, or observe `this`, so the prologue write is
reached before any other effect of the constructor. `PutValueSet` additionally
requires a constant string key and a `This` receiver. Every existing refusal
still applies — derived classes, field initializers, computed keys, parameter
defaults, setter-shadowed fields, and any statement that breaks the leading run.

Behaviour is unchanged: an 11-case semantics probe (unassigned fields still read
`undefined`, `Object.keys`/JSON shape, prototype-setter shadowing, derived
classes, post-construction reassignment, 200-instance shared-shape consistency)
produces byte-identical output before and after, and matches Node on 10 of 11 —
the eleventh is a pre-existing, unrelated divergence in how a declared field
interacts with a same-named prototype accessor, identical on both compilers.

**Not fixed here, and worth its own ticket:** the residual gap is an ordering
one. `lower_call/new.rs` emits `js_gc_init_typed_shape_layout` *after* the
constructor call, so no raw-f64-declared class-field store inside any
constructor can ever pass its guard — the surviving real stores still take
`js_put_value_set`. That belongs with #7510's construction-path item.
