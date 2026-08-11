### Performance

**A subclassed class hierarchy paid a by-name hash store for every field
assignment in every constructor on its chain.** `gc-handoff/apps/shapes.ts`
issued **528 000** `js_put_value_set` calls per run; after this change it issues
**48 000**.

Two independent defects, found in that order, with the second only visible once
the first was fixed.

#### 1. The class-field shape guard bet on the DECLARED class

`expr/class_field_inline_guard.rs`'s inline precheck (and the runtime
`class_field_fast_contract` behind it) compared the receiver's `class_id` and
`keys_array` against the *declared* class of the expression — one pair, exact
match. Inside a base class's own constructor or method that bet is not merely
unreliable, it is **guaranteed wrong**: `this` in `Node2D`'s constructor is only
ever reached through `super(...)` from a subclass, so both compares fail on every
single `this.x = x`, and every inherited read — `Node2D`'s `get originDist`
reading `this.x` — missed 100% of the time.

`class_field_subclass_arms()` collects the base's transitive subclass closure and
the emitter turns the single equality into a disjunction over it, which is the
field-side counterpart of the dispatch widening in
`lower_call/property_get/dynamic_dispatch.rs` (#7800). Soundness does not rest on
the layout algorithm's root→leaf ordering: the field's slot index and its raw-f64
candidacy are **re-derived per candidate subclass**, so a shadowing
re-declaration or an accessor on the subclass chain drops that arm. Capped at 8
arms; a class with no eligible subclass emits **byte-identical IR** to before.

All five `emit_class_field_inline_precheck` sites are widened, **including the
strict BOXED store arm #7854 un-gated**. That one matters on its own: #7854
removed the `requires_raw_f64` gate so boxed declared fields stop paying an
unconditional guard call, but in a base class's own constructor the precheck it
newly emits would still have missed 100% of the time, because `this` there is
only ever a subclass. Its arms are computed with `requires_raw_f64` rather than a
literal, so a candidate subclass whose declared type disagrees about the slot's
representation is dropped.

Measured effect, with per-precondition counters on `shapes.ts`: the runtime get
guard goes from being called on every inherited read to **never being entered at
all** (`get_guard_calls=0`) — every read now takes the inline fast path.

#### 2. #7512, one level up: no subclass instance ever got an at-allocation typed shape

Fixing (1) left the *store* side almost unmoved, and the counters said why —
`contract_cid=0, contract_keys=0, contract_fieldcount=0, set_frozen=0,
set_notplain=0` but `contract_rawf64=144000`. Not the guard's class test: the
side table said the slot was not raw-f64 at all.

`typed_shape::class_layout_declarable_at_allocation` consults
`ctor_prologue_param_assigned_fields`, which returns the empty set the moment a
class has `extends`. Empty prologue ⇒ no `js_gc_declare_typed_shape_layout` at
the allocation site ⇒ `GC_OBJ_TYPED_LAYOUT_INTACT` is clear for the whole
construction ⇒ every raw-f64 field store in every constructor on the chain
misses its guard. That is exactly #7512's mechanism ("declaring the fields
`number` is what makes the class slower — more type information selects a
representation whose guard the construction path has made unsatisfiable"), which
was fixed for a standalone class and never extended past it.

It is not a base-class-only tax: `Node2D` extends nothing, yet its own
`this.x = x` misses too, because the eligibility question is asked of the
**allocated** class. Four TypeScript probes isolate it — a monomorphic class and
a hand-flattened two-field class take the fast path on 100% of constructor
stores; adding a single `extends`, even a *fieldless* one, puts every store on
the chain onto the by-name path.

`chain_prologue_assigned_fields()` answers the same question for a whole chain,
and distinguishes **disqualified** from **qualified but assigns nothing** — the
old single-set API conflated the two, which is precisely what made a chain
unanalysable a class at a time (a fieldless `Marker extends Shape` is the second
case, and is fine). The extra obligations heritage brings:

- A leading `super(...)` is skipped rather than truncating the prologue at
  statement 0, but only when every argument is `This`-free, so the parent
  constructor cannot be handed the half-built instance.
- Every statement **after** a class's prologue run must be a `Stmt::Expr` with no
  `this` anywhere in it — a non-leaf constructor's trailing statements run
  *before* the leaf writes its own fields, so a `this.w` read in `Shape`'s body
  would see a raw-f64-masked slot still holding `undefined`'s NaN-box bits and
  yield `NaN` instead of `undefined`. (`Shape.made = Shape.made + 1` is the
  motivating admission.) The expression scan uses
  `perry_hir::walker::walk_expr_children`, which is exhaustive and drift-checked
  against its `_mut` twin; the statement side is a deliberate **whitelist**,
  because the HIR has no shared statement walker and a missed variant here would
  be a silent wrong answer rather than a missed optimization.
- Every raw-f64 field anywhere on the chain must be prologue-assigned by its own
  class, or the declaration is refused.

The field-init dead-`undefined`-write elision consumes the same chain set exactly
when the chain form is what authorized the declaration. The two must agree: with
the raw-f64 mask live from birth, a field-init `undefined` write into one of
those slots fails `layout_raw_f64_bits` and downgrades the descriptor on the
spot, which would make the declaration worthless.

#### Validation

Compiling the 19-program `gc-handoff` corpus with both arms against the **same**
runtime archives and `cmp`-ing the executables (output basename held constant):
**18 of 19 byte-identical, only `shapes` differs**, and all 19 outputs match
node byte-for-byte with exit 0.

A semantics probe covering the shapes CLAUDE.md flags as weak — fieldless
subclass, indirect subclass, an un-assigned `number` field, a `string` field, and
a post-construction `d.x = "str"` downgrade — is byte-identical to node,
including `Object.keys` order and `JSON.stringify` output. Eight new unit tests
pin the chain analysis, including the two soundness refusals (a trailing
statement mentioning `this`, and a `super()` argument mentioning `this`).
