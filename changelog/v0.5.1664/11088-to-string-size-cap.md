`crates/perry-runtime/src/value/to_string.rs` sat one line under the hard
2000-line cap (`scripts/check_file_size.sh`, a required `lint` step), which
meant any PR that added even two lines to it — including an unrelated one
already in flight — got blocked outright.

Split it into four files under `crates/perry-runtime/src/value/`:

- `to_string.rs` (2000 → 634 lines): the `js_jsvalue_to_string` dispatcher and
  its direct siblings (`js_jsvalue_to_string_method`/`_coerce`,
  `js_ensure_string_ptr`, `js_value_to_str_ptr_for_ffi`).
- `to_string_primitive.rs` (new, 767 lines): the `OrdinaryToPrimitive`/
  `ToPrimitive` resolution machinery for objects, functions, and exotic
  Date/RegExp own-property overrides.
- `to_string_array.rs` (new, 196 lines): `Array.prototype.toString`
  resolution.
- `to_string_radix.rs` (new, 435 lines): radix-based number/BigInt
  stringification (`Number.prototype.toString(radix)` and friends).

This is a pure move: no renamed public items, no behavior changes. Items that
crossed a file boundary got the minimum visibility bump (private → `pub(crate)`)
needed for the split, and every caller that reached into `to_string`'s
submodule path directly (`temporal/options.rs`, `value/dynamic_arith.rs`,
`value/to_string_class_ref.rs`) was repointed at the item's new home instead of
being routed through a forwarding re-export.

Verified by diffing the crate's exported C-ABI symbol set (`nm -g` on the
built `libperry_runtime.rlib`, filtered to non-Rust-mangled names) between this
branch and a pristine `origin/main` checkout: identical, 3196 symbols on each
side, including all six `#[no_mangle]` symbols that live in this file group
(`js_jsvalue_to_string`, `js_jsvalue_to_string_radix`,
`js_jsvalue_to_string_method`, `js_jsvalue_to_string_coerce`,
`js_ensure_string_ptr`, `js_value_to_str_ptr_for_ffi`).
