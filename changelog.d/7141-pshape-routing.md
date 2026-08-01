### Fixed

- **repsel Phase 5a: proven-`this` clones had zero call sites and were dead-stripped (#7128).**
  `collectors/proven_this.rs` emitted a `{method}__pshape` clone — a full second
  body of the method whose `this` carries the `Ptr<Shape>` proof, so every
  `this.field` inside it is a bare fixed-offset `load double` instead of a
  guarded diamond — but nothing in the corpus ever *called* one. Every clone was
  emitted, reachable from nothing, and dropped by the linker.

  The two checks in place could not see it. An object-hash A/B scores the phase
  as working, because `suite_09_method_calls`' object genuinely does differ with
  the analysis off — by two dead clone bodies. And the promotion census scores
  it as working, because the clone's `this` is a real `Ptr<Shape>` consumption,
  recorded at every `this.field` site *inside the body nobody calls*. That is
  the reconciliation of the two contradictory reports: #7117's "7 receiver
  consumptions corpus-wide" and #7128's "zero call sites" are both true and
  describe the same dead code.

  Root cause is arm ordering at both routing sites. `emit_guarded_direct_method_call`
  (`lower_call/method_override.rs`) tried five typed-clone arms and consulted
  `pshape_methods` only in the final `else`; the Phase 3b guard-free site
  (`lower_call/property_get/dynamic_dispatch.rs`) had the same shape, routing to
  the clone on its plain exit but calling the guard-ridden public body from the
  typed-receiver arm's own generic fallback 25 lines above. A method can only
  admit a proven-`this` clone if it touches a declared field of its own chain,
  which is very nearly the definition of a typed-receiver-clone candidate — so
  the typed arm won essentially whenever both were eligible.

  Both sites now resolve the clone once, up front, and use it for the generic
  fallback as well as the plain exit. The typed clone is still preferred on the
  fast path; what changed is that falling off it no longer discards a receiver
  proof the enclosing block had already established. Same `(double this, args…)`
  ABI, same shadow-bound tagged-at-rest receiver slot, no new proof obligation:
  every rerouted block is dominated either by the class-id + keys-token guard or
  by Phase 3b containment, which is exactly what the existing `else` arm already
  relied on.

  New `collectors/proven_this_routing_tests.rs` ratchets **call sites**, not
  symbol presence — it matches the callee position of a `call` so a
  `ptrtoint ptr @…__pshape` operand can never be miscounted — and asserts the
  clone still stores-then-`js_shadow_slot_bind`s its receiver, since
  `GC_TYPE_OBJECT` moves in the shipped configuration (#7019).

  Two findings are deliberately left unfixed and filed instead: routing the
  class-id-switch dispatch tower (`idispatch.caseN`) would be **unsound** —
  `delete inst.field` compacts packed slots while preserving `class_id`, which
  is precisely what the keys token catches — and whether the typed arm should
  yield to the clone on its *fast* path is a cost-model question for the
  `collectors/repsel_benefit.rs` gate added in #7132.

  Measured on a Raspberry Pi 5 (`perry-dev`, one `CARGO_TARGET_DIR` per arm,
  identical package sets): `__pshape` call sites go 0 → 2 on
  `fixture_ptr_shape.ts`, 0 → 2 on `fixture_ptr_shape_sites.ts` and 0 → 1 on
  `09_method_calls.ts`. The body a routed call enters drops from 304 IR lines
  with 4 `js_typed_feedback_class_field_*_guard` and 4
  `js_object_get_field_by_name*` calls to 104 lines with none of either
  (`Point::norm2`, 19 → 7 opaque `js_*` calls). `cargo test -p perry-codegen
  --lib` 409/409, `census --gate` OK with every floor held, and the three
  `Ptr<Shape>`/proven-`this` gap tests byte-identical against `main`.
