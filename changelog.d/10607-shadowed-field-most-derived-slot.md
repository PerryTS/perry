### Fixed

- **A field a subclass overrides could read as the base class's value from
  every read that isn't a compile-time-typed `obj.field`.** `class Sub
  extends Base { tag = "sub-tag" }` (with `Base { tag = "base-tag" }`) gave
  `Sub` two inline field slots named `"tag"` — the packed class layout
  (`crates/perry-codegen/src/codegen/mod.rs`) never deduplicates a name a
  subclass re-declares — but only the most-derived one is ever written
  (`class_field_global_index` already resolves a compile-time-typed
  `obj.field` to that slot, "TS shadowing"). Every *dynamic* by-name lookup —
  an inherited `Object.defineProperty` accessor's `this.field`, a computed
  `obj[key]`, `Reflect.get`, `hasOwnProperty` — instead returned the
  ancestor's never-written slot. Fixed in three runtime lookup sites
  (`object/keys_lookup.rs`, `object/shapes.rs`, `object/field_get_set/ic_miss.rs`)
  to agree with the compile-time path: `keys_find_slot_by_bytes` /
  `keys_find_slot_by_key_ptr` and the IC-miss fast-path scan now walk
  back-to-front, and the ≥32-key indexed lookup keeps the highest matching
  slot index instead of the first. No storage-layout change; validated with
  `test-files/test_gap_10595_inherited_accessor_field_shape.ts` (string and
  Symbol keys, getter+setter, a two-level subclass, and a field overridden
  with a different runtime type) plus new `perry-runtime` unit tests. Still
  open: `Object.keys()`/`for...in` list a shadowed field's name twice, since
  enumeration reads the same undeduplicated keys array — a follow-up.
