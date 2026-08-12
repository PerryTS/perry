### Fixed

- **`instanceof` was false for a monomorphized generic class (#7575).**
  `m instanceof MyMap` was `false` for a `class MyMap<K, V> extends Map<K, V>`
  instance while `m instanceof Map` was `true`.

  Filed as a Map/Set subclass / prototype-chain defect; it is neither. The
  mechanism is **monomorphization**. Perry specializes generic classes, so
  `class Gen<T> {}` plus `new Gen<number>()` emits a second class `Gen$num`
  (`monomorph::mangle::generate_specialized_name`) with its own class id, and the
  instance is stamped with THAT id — while `x instanceof Gen` resolves the RHS to
  the GENERIC's id, which appears nowhere in the specialization's parent chain.
  A generic class over a plain base, and one over no base at all, failed
  identically; `class MyMap<K, V> extends Map<K, V>` is just the idiomatic
  spelling, which is why it surfaced there.

  HIR now records `Class::specialized_from`, codegen emits one
  `js_register_class_generic_origin(spec, generic)` per specialization next to
  the parent edges, and `instanceof`'s chain walk — now a single shared,
  depth-bounded `class_chain_reaches` used by both the static and the
  dynamic-RHS path, which previously had two hand-rolled copies (one uncapped) —
  follows that edge as well as `extends`. It is deliberately a SEPARATE edge:
  `CLASS_REGISTRY`'s chain also resolves `super()` construction, static-method
  lookup and vtable dispatch, so splicing the generic in between a specialization
  and its real base would re-run the wrong constructor.

  This also covers the Array-side sibling #7603 left open — but the note about it
  was stale. Measured on pristine `main`, every NON-generic Array-subclass
  `instanceof` already held (`new MyArr()`, `new Indirect()`,
  `MyArr.from([1,2,3])`); only `new GenArr<number>() instanceof GenArr` was
  broken, and that is fixed here. The stale comment in
  `test_gap_7541_array_subclass_inherited_statics.ts` is corrected in place.

  `test_gap_6325_map_set_subclass.ts` and
  `test_gap_7570_map_set_declared_base_type.ts` are tightened to assert the
  subclass edge, which is why the bug had survived them. New coverage in
  `test-files/test_gap_7575_map_set_subclass_instanceof.ts` (subclass, native
  base, unrelated class, three-level chain, explicit-`super()` subclass,
  iterable-seeded instance, `Symbol.hasInstance` both ways, dynamic RHS, untyped
  parameter, and the generic-over-plain / over-nothing / over-Array shapes with
  their non-generic controls and sibling negatives) plus 4 runtime unit tests
  over the walk — two of which assert the new edge stays DIRECTIONAL, so sibling
  specializations still do not match one another.

  Known and deliberately separate: `constructor.name` still reports the mangled
  `Gen$num` (#7632). Same root cause, different surface, and it can move
  error-message text, so it needs its own parity sweep.
