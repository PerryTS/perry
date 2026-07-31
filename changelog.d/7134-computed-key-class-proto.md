**runtime:** computed / dynamic object-key property access on class prototypes
and class constructors now matches Node (#6945).

Three cooperating gaps:

1. `js_dyn_index_get` treated non-string, non-numeric keys as floats
   (`format!("{}", f64)`), so `obj[{toString(){return "k"}}]` never ran user
   ToPropertyKey. Object / boolean / null / undefined / bigint keys now go
   through `js_to_property_key` (with receiver rooting) before the by-name
   get — matching the set-side path in `js_dyn_index_set`.

2. Class-instance field get walked the reflective decl-proto object only for
   *accessors*, deliberately skipping data reads to avoid class-id re-entry.
   Runtime `C.prototype[k] = v` stores an own data field there, so
   `(new C()).name` missed it while `C.prototype.name` saw it. Own data is
   now read via `own_data_field_by_name` (no re-walk).

3. Codegen's IndexGet last-resort path routes non-string keys on a known
   ClassRef through `js_object_get_index_polymorphic`, which rejected every
   INT32-tagged receiver as a primitive. Registered class-ids now forward to
   `js_dyn_index_get`'s class-ref arm so `C[k]` / `C[objectKey]` resolve
   statics and `CLASS_DYNAMIC_PROPS`.

Regression: `test-files/test_gap_computed_key_class_proto_6945.ts`
(byte-for-byte vs Node 26.5).
