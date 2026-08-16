### Performance

- **`pipeline` 0.9192×, `shapes` 0.8985×, `asyncpipe` 0.9912× (instructions
  retired) — a `.length` read on a receiver codegen could not prove is a string
  paid the whole object property ladder.** `property_get.rs` already emits a
  three-arm string-`length` dispatch (SSO length byte / heap `utf16_len` load /
  property-semantic slow call) and that dispatch is *fully runtime-guarded* —
  it tests the NaN-box tag and only takes an inline arm for a value that IS a
  string. It was gated on `is_string_expr`, a compile-time proof. A receiver the
  front end cannot type (`rec.tag.length` where `rec` is an object-literal type,
  a JSON `any`, an array element) therefore landed in
  `lower_generic_property_get`, where a heap string can never be served: the
  inline cache requires a `GC_TYPE_OBJECT` receiver by construction (#72). Every
  such read missed to `js_object_get_field_ic_miss` and walked a ladder built
  for objects — a closure-magic deref, buffer and typed-array registry probes,
  then `js_object_get_field_by_name`'s own dispatch, which decoded the key with
  `str::from_utf8` again before reaching the string arm. On
  `gc-handoff/apps/pipeline.ts` that one read was 9.7 % of the program as a
  call-graph subtree.

  The generic tower now splits both string tags out at `.length` sites: a heap
  string (`0x7FFF`) loads `utf16_len` at payload offset 0 through the same
  `safe_load_i32_from_ptr` the proven-string lowering uses, and an SSO receiver
  (`0x7FF9`) extracts the inline length byte instead of calling
  `js_object_get_field_by_name_f64`. Everything else keeps the tower unchanged,
  and the split sits after the typed-feedback observation so a mixed
  object/string site still records every receiver. A non-string receiver pays
  one compare and one branch, and only where the key is `length`.

  Sound by construction: a primitive string's `length` is non-writable,
  non-configurable and cannot be shadowed by an own property, and both string
  tags are disjoint from `POINTER_TAG` — this short-circuits a value the runtime
  ladder computed identically. Same shape as #7753 (array `.length` in the miss
  handler) and #7890 (declared array reads reaching the inline `.length`), one
  receiver type over.

  Validated with two compilers against one pinned runtime pair (the change is
  codegen-only; both `libperry_{runtime,stdlib}.a` compare identical): 19/19
  corpus programs exit 0 and are byte-identical to `node`, `cmp` across arms
  reads 14 identical / 5 differ and the 5 are exactly the programs that read
  `.length` through the generic tower, the other 14 measure 0.998–1.0004
  instructions retired, and `iso_miss` still reports `checksum 437840 misses 0`
  under `PERRY_GC_PROTECT_FROMSPACE` and `PERRY_GC_VERIFY_EVACUATION`.
  `test-files/test_gap_dynamic_string_length_generic_tower.ts` feeds the same
  call site a string, an array, array-like objects with numeric and non-numeric
  `length`, a function, a typed array, a number and both nullish values, and
  requires node-identical output including the catchable TypeError.
