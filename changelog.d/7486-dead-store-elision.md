### Codegen: provably-dead per-store bookkeeping elided on class-field stores (#7469)

Profiling `churn.ts` after the hot-TLS work showed the synthesized anon-shape
constructor — executed once per object literal — emitting **four** guarded
field-store sequences for a two-field literal: two real parameter stores, and
two field-default initializations storing a compile-time-constant `undefined`
that the very next statements overwrite. Each dead pair cost a
`js_typed_feedback_class_field_set_guard` FFI call, a
`js_string_addref_if_heap_string` on a constant, and a `js_gc_note_slot_layout`,
20 million times.

Two elisions, both riding existing machinery:

- **Dead default-`undefined` field inits.** `apply_field_initializers_recursive`
  writes `undefined` into every `init: None` field (#486 — `new C().x` must read
  `undefined`, not allocator bytes). When the class's own constructor opens with
  an unbroken run of `this.f = <param>` statements — every synthesized
  anon-shape ctor, and plain user ctors like `constructor(a, b) { this.a = a;
  this.b = b }` — those writes are dead stores: nothing can observe `this.f`
  before the prologue overwrites it. `ctor_prologue_param_assigned_fields`
  proves eligibility (class extends nothing, all fields bare and undecorated,
  all ctor params plain, no setter shadows a prologue field, prologue statements
  are throw-free `LocalGet` assigns) and the init loop skips exactly those
  fields. The `js_object_alloc_class_inline_keys` allocation path already
  pre-fills slots with `undefined` (#4717), making the writes doubly dead — but
  the elision rests only on the allocator-independent prologue guarantee.
- **Value-side elision on the guarded store path.** The guarded class-field
  store already elided the write barrier for values that are non-pointers by
  construction (#5334 lever D) but hardcoded the string-addref and layout-note
  on. The Phase 4b.1 predicates are value-side-only proofs — safe in every
  receiver layout state per their own documentation — so they now gate all
  three calls. `this.count = 0` on the guarded path emits guard + raw store and
  nothing else.

Semantics probed against Node byte-for-byte across the edge cases (unassigned
fields still read `undefined`, param defaults and pre-assignment side effects
refuse the elision, setter-shadowed fields refuse it, `Object.keys`/JSON shape
unchanged). GC ratchet vs `main` agrees on 107 of 108 deterministic metrics
(the one difference is a −288-byte `heap_used_bytes` allocation-boundary
wobble); collector accounting is identical on all 12 probes.
