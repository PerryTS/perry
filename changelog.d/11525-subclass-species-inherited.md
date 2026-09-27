A user subclass of a built-in now inherits the built-in's
`get [Symbol.species]` accessor: `class X extends Array {}` (and `Map`, `Set`,
`Promise`, `RegExp`, `ArrayBuffer`, `Uint8Array`, …, including indirect
subclasses and class expressions) answers `X[Symbol.species] === X`. #11338
installed the accessor on the built-in constructors but left this case
`undefined`: a class ref's symbol lookup walked user ancestors, class-expression
parents and function parents, but never the built-in constructor its chain
ends in, because a built-in parent is recorded only as a reserved class id.
The lookup now takes one more step, `builtin_parent_ctor_in_chain`
(`object/class_registry/state.rs`). It reads the parent value
`js_register_class_parent_dynamic` already stashes at definition time and reads
the symbol off that constructor with the original receiver, so the inherited
getter answers the subclass. Prototype refs are excluded. This is the last open
part of #11193.

With the species found, RegExp `split` and `matchAll` construct their matcher
through the subclass. That exposed an older gap. A no-own-constructor class
whose chain ends at an exotic built-in (`RegExp`, `ArrayBuffer`, the typed
arrays) has a synthesized standalone constructor, and that constructor skipped
the built-in as an uncallable base. So every dynamic construct of such a class
(`new (R as any)(…)`, `Reflect.construct`, a species `Construct`) got a plain
object with no `[[RegExpMatcher]]` or buffer, and `"a,b".split(new R(","))`
would have thrown "RegExp builtin exec requires a RegExp receiver". The
synthesized constructor now emits the built-in's own Construct with the class as
newTarget (`js_builtin_subclass_construct`, `codegen/method.rs`), matching the
inline `new R()` lowering and an explicit `super()`. The walk is
`exotic_builtin_base_in_chain` (`lower_call/new_helpers.rs`), which shares the
ctor-less walk with `native_instance_base_in_chain`. Class fields still
initialize on the constructed instance.

Still open, and unchanged by this: the Array and typed-array species consumers
(`map`/`filter`/`slice`, …) still return plain results for subclass instances,
because their default fast paths never consult `constructor`. A dynamic
`new (A as any)(3)` of an `Array` subclass still has length 0.

Covered by `test-files/test_gap_11193_subclass_species_inherited.ts`.
