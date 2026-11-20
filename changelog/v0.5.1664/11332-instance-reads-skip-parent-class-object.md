Fixed instance reads on a class that extends a factory-returned class object
(#10890).

A class declaration that extends a class OBJECT (Effect's
`class InitError extends Schema.ErrorClass(...)(...) {}`, whose `makeClass`
returns `const out = class extends Inherited { … }`) records that parent
class object in `CLASS_PROTOTYPE_OBJECTS` (#1788). Static reads on the
subclass use it to inherit the parent's statics. The instance-side walks read
the same entry as if it were a prototype, so an instance:

- read the parent's statics (`init.identifier`, `typeof init.make`,
  `init.ast`), where Node answers `undefined`;
- read `name` as the class binding's own name (`out`) instead of the `name`
  written to `out.prototype`;
- missed `Object.assign(out.prototype, { tag, greet() {} })` entirely (`tag`
  read `undefined` and `greet()` threw "not a function");
- read symbol-keyed statics (`init[TypeId]`, `TypeId in init`). Effect's
  `Schema.isSchema(u)` is `TypeId in u`, so every such instance passed it.

The instance walks now skip that entry and read the parent evaluation's
materialized prototype object (non-allocating: an unmaterialized prototype
cannot hold a user write). This covers the property walk
(`resolve_proto_chain_field_inner`), the vtable walk for instance method
calls, and the symbol chain walk (split by whether the receiver is a class
object). A synthetic id's entry (`Object.create(C)`) is a real prototype and
is unchanged.

Coverage: `test-files/test_issue_10890_parent_class_object_reads.ts`
reproduces Effect 4's `ErrorClass` shape plus plain, symbol-keyed and
override variants; it fails on the previous runtime and matches Node here.
`prototype_objects::parent_class_object_tests` asserts the instance and
constructor sides of the same edge, plus the `Object.create(C)` case, and
fails when the new predicate is disabled.

The published `effect` 4.0.0 betas that still export `Schema.TaggedErrorClass`
currently fail to link (undefined `__perry_wrap_perry_fn_…_Array_js__Array_`),
which is a separate bug. The fixture reproduces the package's class shapes
instead.
