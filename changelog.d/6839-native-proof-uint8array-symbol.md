### Fixed

- **`native_owned_uint8array_get_fallback_uses_uint8array_helper` has been red
  on `main` since #6092.** That PR moved the unproven-bounds JS-value read off
  the i32 `js_uint8array_get` accessor — whose out-of-range answer is the `0`
  byte-sentinel — onto `js_uint8array_index_get_value`, which reads `undefined`
  per IntegerIndexedExotic `[[Get]]` (#6088). The proof still asserted the old
  symbol. What the test is about does not change: a disposed native Uint8Array
  view must fall back through the Uint8Array-shaped accessor, never the
  Buffer-shaped one.

  The sibling inline-path test forbade only `js_uint8array_get`, so after #6092
  a regression that took the slow path would have gone unnoticed. It now
  forbids both helpers.

  Integration suites under `crates/*/tests/` don't run per-PR (#5960), which is
  why this sat red — it surfaces only when a PR touches perry-codegen and pulls
  the suite into `e2e-scoped`.
