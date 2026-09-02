### Fixed

- **A rejected strict `arr.length = n` now throws when `length` is non-writable
  by descriptor, not only when the array is frozen.**

  ```js
  "use strict";
  const a = [1, 2];
  Object.defineProperty(a, "length", { writable: false });
  a.length = 0;   // node: TypeError   Perry: silent (a.length stayed 2)
  a.length = 2;   // node: TypeError   Perry: silent  -- a same-value write is rejected too

  const b = [1, 2]; Object.freeze(b);
  b.length = 0;   // node: TypeError   Perry: TypeError (already correct)
  ```

  ES2024 §6.2.5.7 (`PutValue`) calls `Set(O, "length", n, Throw)` with
  `Throw = IsStrictReference`, and `OrdinarySet` consults `length`'s own
  descriptor and reports `false` **before** it looks at `n` — so a non-writable
  `length` rejects even a write of the value it already holds.

  `js_array_set_length_strict` recognised only ONE of the two ways `length`
  becomes non-writable. It tested `OBJ_FLAG_FROZEN`, which `Object.freeze` sets;
  an explicit `Object.defineProperty(arr, "length", { writable: false })` records
  the attribute in the descriptor side table **without** freezing the array, and
  that shape fell straight through to the sloppy body — whose own non-writable
  arm is a silent `return`, annotated "strict-mode throw is handled by the
  caller's `PutValue`". This entry *is* that caller. The throw set and the no-op
  set had drifted apart, and nothing tied them together.

  The predicate is not new: `array_length_is_non_writable` is what
  `push`/`pop`/`shift`/`unshift` have guarded with since test262
  `Array.prototype.{push,pop,shift,unshift}/set-length-*-non-writable` — those
  mutators perform the same `Set(O, "length", …, true)`. `js_array_set_length_strict`
  was the one such site not using it. It is now checked **before** the
  zero-truncate fast path, so a write the spec rejects cannot reach a shortcut
  that stores.

  Scope, stated because the neighbouring cases look similar and are not fixed:
  `Object.seal` and `Object.preventExtensions` leave `length` **writable**, so
  they are not this rejection and do not throw here. Perry's handling of those
  two is wrong in a different, non-strictness way — it refuses the length change
  outright, in both modes, where node performs it (`preventExtensions` then
  `a.length = 5` gives 5 in node, 2 in Perry) — and a sealed shrink should reject
  via ArraySetLength's deletion walk, which Perry does not model. Making the
  strict entry mirror the sloppy body wholesale would have turned both of those
  wrong answers into wrong TypeErrors, so it deliberately does not.

  `test-files/test_gap_9422_strict_object_store_strictness.cts` is a `.cts`, so it
  is a CommonJS script in BOTH runtimes, with a sloppy arm and a `"use strict"`
  arm. BOTH ARMS ARE ASSERTED, across the seven rejection shapes — frozen,
  sealed, non-writable own, non-writable inherited, getter-only own, getter-only
  inherited, non-extensible — plus the computed-key, class-field, update and
  array-`length` lanes, and the over-throw controls (`sealed` and
  `preventExtensions` writes to an EXISTING property, and an inherited setter,
  all of which succeed in both modes). Byte-compared against node 26.5.1.

  Unit test: `set_length_rejection_throws_only_in_strict_mode` in
  `crates/perry-runtime/src/array/strict_store_tests.rs`, beside #9394's
  `element_store_rejection_throws_only_in_strict_mode`, asserting both arms and
  the writable-`length` control.

  **What #9422 as filed claimed, and what is actually true.** The issue reported
  that `"use strict"; const o = {x:1}; Object.freeze(o); o.x = 9;` is silent in
  Perry, and located the cause as codegen emitting
  `js_put_value_set(..., strict = 0)` at *every* property-set site. Neither holds
  on `main`. That two-line program throws correctly, and so does every other
  ordinary-object shape tested above. The emitted IR shows why: the strict arm
  lowers to `js_class_field_set_fallback` (which throws), while the two
  `strict = 0` literals in `expr/property_set.rs` sit inside
  `try_lower_sloppy_class_field_store` / `…_boxed_store`, which
  `expr/proxy_reflect.rs` reaches only under `if !*strict` — where `strict = 0`
  is the correct constant. The array-`length` lane above is the one place a
  rejected strict write really was silent.
