Completed the class-semantics tail from #5893 (167/167 on the issue's Test262
worklist): derived construction and `super` for dynamic functions and native
built-in subclasses (return overrides, `new.target`, prototype identity),
private instance/static element branding and dispatch across fresh class
evaluations, proxies, accessors and extracted methods, and the remaining
computed/static class element, property-key and constructor/prototype corners.
Concentrated class runtime logic was split into focused source fragments to stay
under the 2000-line file cap.

Two spec-ordering fixes are worth calling out because they change observable
behaviour:

- `Object.defineProperty` no longer runs `[[Set]]`. `define_property_force_store_value`
  used to funnel through `js_object_set_field_by_name`, which performs inherited
  setter lookup; DefineProperty must write the receiver's own slot. It now
  ensures the shape entry exists and stores by slot index (or through overflow
  storage).
- `[[Set]]` honours OrdinarySet step 1: an own data property on the receiver
  shadows an inherited class accessor, so the class-vtable setter walk in
  `set_field_by_name_object_tail` only applies when the receiver has no own
  property of that name. Node 26.5.1, on the exact production path
  (`Object.assign` funnels into `js_object_set_field_by_name`):
  `class A { x = 1; set x(v){} }` + `Object.assign(a, {x: 7})` stores 7 and
  never calls the setter, while the same assignment on a class that only
  declares the accessor still dispatches it (the #486 hono `set res(_res)`
  shape). `typed_feedback_class_field_set_guard_falls_back_for_class_setter`
  asserted the pre-spec behaviour and now covers both halves. The own-key probe
  is evaluated last and only on the slow path, so a store-plan hit does not pay
  for the keys-array scan.

Follow-up fixes on top of the original branch:

- `normalize_swc_class_syntax`'s tokenizer walked `masked.as_bytes()` and
  advanced its cursor by a raw byte, then sliced the `&str`. Ordinary source
  such as `const re = /a€b/;` panicked with "byte index N is not a char
  boundary". It now advances by whole characters; a regression test covers a
  non-ASCII regex literal, identifier and array literal, plus the
  source/masked boundary map with non-ASCII text ahead of a rewritten token.
- `native_module.rs` had grown to 2018 lines, over the repository cap. The
  class constructor/prototype ref values and their prototype-method lookups
  moved to `native_module/class_ref_values.rs` (textually `include!`d, like
  `class_method_values.rs`, so module paths and visibility are unchanged).
- #7341 shapes restored: `define_property_force_store_value` re-reads the
  receiver and key through nested `across_mut`/`across_const` around
  `ensure_key_in_keys_array` instead of two bare handle reads, and
  `js_weak_collection_subclass_init` passes the entries array through
  `with_mut_ptr` as a scoped argument. Raw-handle debt back to 925.
- The two new open-coded `StringHeader` payload offsets use the existing
  readers (`string_key_eq`, `crate::string::string_data`) rather than
  re-deriving the offset.
- Three new `perry_thread_local!` holders are pinned on the GC root-holder
  frontier ratchet with their research: `PRIVATE_METHOD_OWNER_HINT` and
  `PRIVATE_MEMBER_ACCESS_HINTS` hold only `u32`/`String`/`bool` owned data, and
  `DERIVED_SUPER_BINDING_STACK` holds the derived constructor's own i1 ALLOCA
  address — a native stack address, which does not move under GC — bounded by
  its push/pop and savepoint/restore pairs.
