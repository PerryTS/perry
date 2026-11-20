Restored the optimized store for class field initializers.

#8630 replaced the field-initializer lowering with an unconditional
`js_class_field_add` — a full `[[DefineOwnProperty]]` behind a handle scope,
per field, per construction. The motivation was correct: DefineField uses
CreateDataProperty, so an inherited setter must not run and a Proxy receiver
must observe its `defineProperty` trap. But a bare declaration (`x: number;`)
still gets a synthesized `undefined` initializer, so an ordinary class pays the
full define path for every field of every instance. `shapes.ts` (7 classes,
~2-3 fields each, 120k constructions) measured **3.11x** the instruction count
of the pre-#8630 compiler.

The two semantics coincide when neither difference can arise, and both are
statically decidable:

* **no accessor anywhere on the chain** — `class_field_global_index` already
  answers exactly this, returning `None` the moment an accessor or a
  re-declaration appears on the chain (the #5654 machinery); and
* **the receiver is provably the freshly allocated ordinary instance** — no
  constructor on the chain returns a value, `js_ctor_return_override` being the
  only route by which a Proxy can become the field-initializer receiver.

When both hold, lower through the `PropertySet` path (inline shape precheck ->
direct slot store) as this did before #8630. Everything else keeps the full
DefineField call. The chain walk is conservative on every edge it cannot see: a
native base, a dynamic `extends`, an id-only parent edge, or a class missing
from `ctx.classes` all answer "unsafe".

Measured (instructions retired, vs the pre-#8630 compiler at `00bddb34b`):
`shapes` 3,662,616,604 -> 1,320,393,329 against a 1,179,031,124 baseline —
**94% of the regression recovered**, 3.11x -> 1.12x.

Verified against Node 26.5.1 that #8630's fix is preserved: for
`class Base { set v(x) {...} }` / `class Derived extends Base { v = 5 }`, the
pre-#8630 compiler printed `SETTER RAN` and `d.v=undefined`; this prints
`d.v=5`, matching Node. A getter two levels up the chain also matches, and a
value-returning parent constructor still emits `js_class_field_add`.

Partial for #8648: `cycles`, `deeplist` and a two-class `new B(x, y)` loop are
essentially unmoved by this, so they have a second, independent cause.
