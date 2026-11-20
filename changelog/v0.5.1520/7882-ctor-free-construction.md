### perf(codegen): construct field-only objects without calling a constructor

An object literal with a closed shape is not lowered as a literal. HIR mints an
anon-shape class for it and rewrites the site to `new __AnonShape_<hash>(v, w)`,
and `lower_new` routes that — like every own-constructor class — through the
shared standalone `<Class>_constructor` symbol. So `{ v, w }` and
`class Node { constructor(v, w) { this.v = v; this.w = w } }` compile to the same
thing: a bump allocation whose header is a compile-time constant, followed by a
call into a symbol where `this` is an **opaque parameter**.

Being opaque is the cost. Every `this.f = p` inside that symbol emits the full
class-field precheck — a volatile load of the policy latch, seven header loads,
nine compares and a two-block diamond — per field, per object. And every one of
those conditions is a constant the *caller* wrote three instructions earlier:
`typed_layout_baked` (#7834) certifies `GC_TYPE_OBJECT`, not-forwarded,
`OBJECT_TYPE_REGULAR`, the class id, the field count, the keys-array pointer, no
per-object descriptors, not-frozen, and `GC_OBJ_TYPED_LAYOUT_INTACT` — all
stamped into the packed header the inline bump allocator emits.

So for a class whose entire constructor is a run of `this.<field> = <parameter>`
stores, the call is avoidable: store the fields at the `new` site and skip it.
Two things are still decided at runtime, but **once for the construction instead
of once per field** — the sticky `PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED` latch,
and whether every value is a plain finite number. A single non-number sends the
whole construction to the unchanged constructor call, so no field is ever stored
before the decision is made.

The bits stored are identical to what the boxed path would write: a JS number's
NaN box *is* its double bits, and the finite test rejects every NaN-box tag
(INT32-boxed integers included, since they share the all-ones exponent). That is
also why no `js_array_numeric_value_to_raw_f64` canonicalization is needed — the
only inputs that helper rewrites are exactly the ones the finite test rejects.

Deliberately narrow, and every refusal is a thing the constructor symbol does
that this path does not reproduce: any heritage, any accessor, decorators,
computed members, initialized/private/computed-key fields, non-plain parameters,
an argument count that is not exactly the parameter count (a capture-carrying
constructor appends `__perry_cap_*` arguments), or a body that is anything other
than the full run covering every declared field exactly once. Partial coverage
would leave a declared raw-f64 slot holding the allocator's `undefined` fill
under an INTACT header, which is precisely the state
`layout_pointer_free_at_allocation` exists to prevent.
