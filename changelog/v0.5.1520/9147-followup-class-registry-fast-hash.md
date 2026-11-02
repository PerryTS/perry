### Changed

- **The class-id-keyed registries moved off SipHash.** #9147 converted the
  pointer-keyed runtime registries to `fast_hash`; the class registries were
  the sampled remainder. A symbolized 12-run profile of `claude-code --help`
  on the post-#9147 tree put SipHash (`RandomState` on the stack) at 3.08% of
  on-CPU samples, and named these callers: `get_parent_class_id` (0.198% of
  total samples), `remember_class_keys_array` (0.175%),
  `js_gc_typed_shape_id_for_keys` (0.115%), plus the `js_register_class_*`
  family (~0.18% combined).

  Converted to `PtrHashMap` / `PtrHashSet`:

  * every class-id-keyed table in `ClassImageTables` — `vtables`,
    `static_methods`/`static_accessors` (OUTER map only), `constructors`,
    `constructor_flags`, `registered_class_ids`, `parents`,
    `fetch_parent_kind`, `generic_origin`, `extends_error`, `has_instance`,
    `to_string_tag`, `extends_data_view`, `extends_typed_array`, `names`,
    `lengths`, `anon_shape_class_ids`;
  * `CLASS_KEYS_BY_ID` (`object/alloc.rs`), probed on every object
    construction — `js_build_class_keys_array` calls
    `remember_class_keys_array` even on its shape-cache hit path;
  * `ObjectHotTables::shape_cache_overflow`, probed whenever a shape id misses
    the 256-entry direct-mapped inline cache (ids step by `10007 mod 256 == 23`,
    so any image with >256 live shapes collides constantly). Every sibling
    field of that struct was already a fast table or an `UnsafeCell` array;
    this one was the exception.

  `gc::layout::typed_shape`'s `ids_by_layout` takes `FastKeyHashMap`, not
  `PtrHashMap`: `RegisteredTypedShapeKey` is a composite of two `u32`s and two
  `Vec<u64>` mask lists, and `PtrHasher`'s overwriting `write_*` would collapse
  it to its last field. Its sibling `layouts_by_id` is a bare `u32` key and
  takes `PtrHashMap`.

  Every key here is a codegen-minted class id or a runtime-minted shape id —
  never external input — so SipHash's DoS resistance buys nothing.

  **Deliberately left on SipHash**, because their keys are JS-supplied
  property/member names on paths that can see adversarial input, or because
  their iteration order is user-visible: the INNER `HashMap<String, _>` of
  `StaticMethodTable`/`StaticAccessorTable`, `method_bind_lengths` and
  `static_method_bind_lengths` (`(u32, String)`), `CLASS_DYNAMIC_PROPS`' inner
  `HashMap<String, f64>` (its `.keys()` feeds `Object.keys(C)`, and
  `sort_property_names_ecma` orders only the integer-like keys — string keys
  keep map order), `ClosureProps::values`, and `js_for_in_keys_value`'s
  shadowing `HashSet<String>`.

  Iteration-order audit: none of the converted maps is iterated anywhere except
  GC root scans (`.values_mut()`, commutative) and `class_side_table_root_snapshot`;
  `CLASS_KEYS_BY_ID`, `shape_cache_overflow`'s non-GC paths, `ids_by_layout` and
  `layouts_by_id` are never iterated at all. `cc --help` output is byte-identical
  before and after.

  Measured on an otherwise idle Mac mini (load 1.6), alternating A/B, 25 paired
  rounds of `claude-code --help`: median 873.0 ms -> 867.0 ms (**-0.69%**), min
  871 -> 862 ms, opt faster in 25/25 rounds (mean delta -6.2 ms, stdev 2.2).
  In the profile, `get_parent_class_id`, `js_gc_typed_shape_id_for_keys`,
  `shape_cache_insert` and the `js_register_class_*` family no longer appear
  under any SipHash frame at all. `.text` grows ~2 KB (+0.016%).
