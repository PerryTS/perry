**GC rooting: the `js_string_coerce`-as-property-key family (#6943).** Third and last known family in
the unrooted-operand-across-GC-capable-coercion series (after #6934's dynamic arith and #6941's
`ToPropertyKey`). A set of property entry points stringify the key with `js_string_coerce` directly,
without an earlier `ToPropertyKey` — and held the receiver, and on the write paths the value about to
be stored, as raw Rust locals across it. `js_string_coerce` allocates for every key shape except an
already-heap `STRING_TAG` one (an SSO short key materializes onto the heap, a numeric key builds its
stringification, an object key runs a user `toString`/`valueOf`), and an allocation can trigger a GC
that **evacuates** live objects. A Rust local is neither a GC root nor a shadow slot, so a stale
receiver dropped the write onto a forwarding stub and a stale stored value planted a dangling pointer
inside a live object, where it outlived the call.

Fixed with the established idiom — `crate::gc::RuntimeHandleScope` plus
`root_heap_word_u64`/`root_raw_mut_ptr`/`root_nanbox_f64`/`root_string_ptr`, re-reading each operand
through its handle after the coercion — across:

- `object/object_ops/define_property.rs` — `Object.defineProperty`'s closure, typed-array and
  ordinary arms (the receiver, the descriptor, and the already-dereferenced `closure_ptr` /
  TypedArray address / `ObjectHeader`).
- `object/descriptors.rs` — `getOwnPropertyDescriptor`'s class-object / typed-array / closure /
  ordinary arms, `string_primitive_descriptor` (whose receiver is itself a movable heap string), and
  `getOwnPropertyDescriptors` (result receiver + each stored descriptor).
- `object/descriptor_state.rs` — `reflect_getter_closure_bits`, including the prototype-walk cursor.
- `object/reflect_support.rs` — `obj_value_has_own_key`'s three arms plus `obj_value_attrs`, where a
  stale receiver **address** silently misses the descriptor side table instead of crashing, so a
  `Reflect.defineProperty` on a non-configurable property could slip through.
- `object/array_object_ops.rs` — `array_length_reflect_define`.
- `object/typed_array_define.rs` — `typed_array_own_index` and `typed_array_define_own_property`. The
  issue named the shared `canonical_index_for_key` helper; reading it showed the helper is clean and
  the hazard is at these two callers, which resolve the view address before the coercion and
  dereference it after.
- `proxy.rs` — the store fast path in `ordinary_set_with_receiver` (`obj.f = v`). The issue named the
  class-instance arm; the plain-object arm reaches the same coercion transitively through
  `object_proto_may_intercept_key` → `obj_value_has_own_key`, so one optional scope now covers both.
  An inert-key check keeps the common already-heap-string key on the pre-fix code path verbatim.
- `object/object_ops/has_own.rs`, `object/native_call_method/common_methods.rs` —
  `hasOwnProperty` / `propertyIsEnumerable` in both their entry-point and method-call forms.
- `object/object_ops/from_entries.rs` — `Object.fromEntries` (fresh result receiver **and** the entry
  value written into it).
- `symbol/properties.rs` — `js_class_register_static_symbol`'s non-symbol arm (the stored payload).
- `object/with_env.rs`, `error.rs` — the `globalThis`-by-name helpers, whose window spans a read, a
  `ToNumeric`/step, and a write-back.

New shared predicate `builtins::string_coerce_is_inert(value)`, the `js_string_coerce` analogue of
#6941's `property_key_coercion_is_inert`, justified by `js_string_coerce`'s own `is_string()` early
return. Hot surfaces are gated on it so an already-heap-string key pays nothing.

`proxy.rs`'s `target_set` was audited and is provably inert (its argument is always a
`js_to_property_key` result, i.e. an already-heap string); a comment now records that so the next
sweep doesn't re-examine it. `crates/perry/tests/gc_string_coerce_property_key_rooting_6943.rs` adds
three forced-evacuation behavioral guards. As with both predecessors, no deterministic pre-fix
failure is reachable from compiled code today — `gc()` runs a full mark-sweep and pins raw locals via
the conservative stack scan (#6946), and `perry/gc`'s `minor()` engages the same scan (#6942) — so
the suite is a guard, not a red-to-green regression test, and its module doc says so.
