**A mixin applied to a mixin no longer crashes.** `const Mixed2 = mixin(Mixed)`
— the second level of a mixin chain — SIGSEGVed as soon as anything derived
from it was constructed (`exit 139` where node printed the value). Mixin
composition is normally written as a chain, so this was the common shape
rather than an edge case, and it was a crash rather than a wrong value.

The HIR mixin fast path synthesizes a real class for `const M = mixinFn(Base)`.
At the second level the base is a lexical VALUE binding, so the parent is
correctly captured as `extends_expr` — a dynamic parent — instead of a static
class link. The missing half was the registration: unlike the sibling
`const X = class {…}` path in the same function, this arm bound the
synthesized class without emitting the declaration-time
`RegisterClassParentDynamic`. Its constructor therefore asked
`js_get_dynamic_parent_value` for a class id nothing had registered, and with
an undefined parent `js_fetch_or_value_super` fell back to the most-derived
receiver, re-selected the same class, and recursed until the stack overflowed.

The registration is now emitted here too, in source order after the parent's
own value binding and before the synthesized class's. A single-level
`mixin(Root)` extends a real class, keeps `extends_expr` at `None`, and is
unchanged — which is why one level already worked (#9073) and two did not.

Pinned in both directions. The gap fixture keeps the issue's reproducer
verbatim and adds what it left open: inherited base state and the mixin method
through both synthesized levels, `instanceof` across the whole chain, a leaf
with no own constructor, the one-level case #9073 fixed, and a three-level
chain built from three distinct mixins so a dropped level shows as a missing
method rather than being masked by identical bodies. A lowering unit test
asserts the registration lands between the parent's binding and its own, and
asserts the negative for level 1; it was confirmed to fail against the
unpatched lowering. The compiled fixture's LLVM now has a matching
`js_register_class_parent_dynamic` for every `js_get_dynamic_parent_value` in
the module — zero orphans.
