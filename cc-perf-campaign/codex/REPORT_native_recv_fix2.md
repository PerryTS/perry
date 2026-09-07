# Native-call receiver follow-up 2

## Implementation

- Code SHA: `bf1d91892cf5434641a53aed7256c4d4bb4c96b9`
- Branch: `fork/perf/native-call-receiver-class`
- The code SHA was confirmed remotely with:

  ```text
  bf1d91892cf5434641a53aed7256c4d4bb4c96b9	refs/heads/perf/native-call-receiver-class
  ```

## Exact remaining probe path

The site cache was not stale. `js_native_call_method_at_site` classifies the
boxed receiver at `crates/perry-runtime/src/object/native_call_method.rs:1410`;
`classify_native_receiver` reads the tracked `GC_TYPE_OBJECT` and validates the
site-cache hit at `:151-160`. Its headerless Buffer and typed-array fallbacks at
`:170` and `:174` are therefore unreachable for this receiver.

The residual probe was an internal Array element read downstream of that hit:

```text
js_native_call_method_at_site
  -> try_url_search_params_dynamic_dispatch       native_call_method.rs:1708
  -> shape_is_url_search_params                    search_params.rs:1135
  -> js_array_get_f64(keys_arr, 0)                 search_params.rs:1026
  -> byte-storage redispatch                       array/indexing.rs

and later:

js_native_call_method_at_site
  -> own method-name scan                          native_call_method.rs:2383
  -> js_array_get(keys, i)                         native_call_method.rs:2384
  -> js_array_get_f64                              array/jsvalue_api.rs:17
  -> byte-storage redispatch                       array/indexing.rs
```

At parent SHA `4f257d3b4`, `js_array_get_f64` called
`lookup_typed_array_kind` unconditionally at
`crates/perry-runtime/src/array/indexing.rs:579`, then called
`is_registered_buffer` unconditionally at `:586`, after `clean_arr_ptr` had
already returned a validated internal `GC_TYPE_ARRAY`. The exact Buffer
registry admission is the test-counter increment at
`crates/perry-runtime/src/buffer/header.rs:531` followed by
`is_registered_buffer_slow` at `:532`. The typed-array equivalent increments at
`crates/perry-runtime/src/typedarray/mod.rs:459` and can reach
`lookup_registered_typed_array_kind` at `:501`.

The 5-8 Buffer increments are repeated reads of managed internal arrays during
the URLSearchParams shape/backing probes and subsequent generic method and
prototype resolution. The varying count is address-window and cache-state
dependent; all of the admitted managed addresses are already classifiable from
GC metadata.

The receiver's own property named `buffer` cannot require a Buffer registry
question. The receiver is a tracked `GC_TYPE_OBJECT`; its property-name store
is a tracked `GC_TYPE_ARRAY`; and the field value is the number `17`. The string
`"buffer"` is only compared with the requested method name `"toString"`; the
value is not loaded because the names do not match. A property spelling never
changes either allocation's storage layout or GC type.

## Complete byte-storage call audit on the cached-hit route

- `canonicalize_bare_gc_receiver` returns at
  `object/native_call_method/bare_receiver.rs:114-115` for this NaN-boxed
  receiver. Consequently the calls to `is_registered_buffer`,
  `is_uint8array_buffer`, and `lookup_typed_array_kind` at `:175-178` do not run.
- The receiver classifier's `is_registered_buffer` and
  `lookup_typed_array_kind` calls at `object/native_call_method.rs:170` and
  `:174` do not run because the tracked header and cached `(site, GC type,
  class id)` answer at `:151-160` succeeds.
- The remaining calls that did run were the `js_array_get_f64` redispatches
  identified above: old `array/indexing.rs:579` and `:586`. Their slow callees
  are `typedarray/mod.rs:501` and `buffer/header.rs:532`, respectively.
- `get_field_by_name_object_tail` has syntactic registry fallbacks at
  `object/field_get_set/get_field_by_name_tail.rs:282` and `:453`, but its
  tracked type at `:270-271` is `GC_TYPE_OBJECT`, so neither executes.
- Date/Temporal/Promise defensive checks can call
  `typedarray::is_offheap_sidetable_alloc`; that helper returns on the tracked
  header at `typedarray/mod.rs:541-542`, before its
  `lookup_typed_array_kind` / `is_registered_buffer` pair at `:544`.
- The final Object brand derives `tracked_type` at
  `object/to_string_tag.rs:158-160`. Its `is_registered_buffer` fallback at
  `:208` and `lookup_typed_array_kind` fallback at `:245` both require
  `tracked_type.is_none()`, so neither executes for the plain object.
- `Object.prototype.toString` reads `@@toStringTag` with
  `symbol::own_symbol_property` and `symbol::inherited_symbol_property`; those
  helpers do not call a byte-storage registry. The general symbol getter's
  Buffer/typed-array fallback is not used by this route.
- No `is_uint8array_buffer` or `is_uint8array_buffer_slow` call executes on the
  cached plain-object route. The only syntactic one in the entry funnel is the
  unreachable bare-receiver call noted above.

## Change

`crates/perry-runtime/src/array/indexing.rs:584-600` now reads the resolved
array address's allocator-tracked GC header once. A managed
`GC_TYPE_TYPED_ARRAY` or `GC_TYPE_BUFFER` is dispatched directly from that
type; every other tracked type is an authoritative negative. Only a
tracked-header miss may consult the typed-array or Buffer side registry, which
preserves headerless external Buffer, SAB backing, and native-view behavior.

For the object's internal keys arrays, `tracked_type == GC_TYPE_ARRAY`, so both
registry expressions short-circuit. The assertion and its fixture were not
changed, and no temporary counters or caller instrumentation remain.

## Verification

Immediately before the Cargo decision:

```text
Filesystem     1G-blocks Used Available Capacity
/dev/disk3s1s1       926   15         0    98%
```

This is below the binding 12 GB floor. No Cargo command was invoked and no wait
for disk recovery was performed.

Completed non-Cargo checks:

```text
rustfmt --edition 2021 crates/perry-runtime/src/array/indexing.rs — exit 0
git diff --check — exit 0
```

`python3 scripts/addr_class_inventory.py` was also run. It failed on existing
branch findings outside this patch: the frozen count in
`object/to_string_tag.rs` is 11 versus 10, and
`intl/segments_view.rs:101` has an allowlist-unregistered GcHeader cast. This
patch changes only `array/indexing.rs` and introduced neither finding.

Not run because of the disk gate:

- `cargo test -p perry-runtime --release --lib -- --test-threads=1`
- the archive feature-set build (`cargo build --release -p perry-runtime --features wasm-host`)
- `cargo build --release -p perry`

In particular, the post-fix exact test and full release suite were not run
locally; verification here is static.
