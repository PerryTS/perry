# Native-call receiver follow-up 3

## Implementation

- Code SHA: `6870b1db3eb85db3ae8c30fa6c79a814c162bcdc`
- Branch: `fork/perf/native-call-receiver-class`
- The code SHA was confirmed remotely with:

  ```text
  6870b1db3eb85db3ae8c30fa6c79a814c162bcdc	refs/heads/perf/native-call-receiver-class
  ```

## Exact remaining probe path

At parent SHA `80c58182d`, the one remaining Buffer probe was the call at
`crates/perry-runtime/src/object/map_set_subclass.rs:71`:

```text
buffer::header::is_registered_buffer
  <- object::map_set_subclass::instance_object_ptr
  <- object::native_call_method::collection_methods::dispatch_map_set
  <- object::native_call_method::js_native_call_method_at_site
  <- cached_plain_object_receiver_probes_zero_buffer_registries
```

`js_native_call_method_at_site` classified the receiver correctly and got a
site-cache hit, but it then entered `dispatch_map_set` unconditionally.
`dispatch_map_set` asked `subclass_backing_of`, whose `instance_object_ptr`
excluded Map, Set, Buffer, and typed-array storage by querying all four side
registries before trying to interpret the receiver as an ObjectHeader. Thus an
allocator-tracked plain object paid exactly one Buffer-registry admission.

The first `toString` call paid the same unconditional probe. The test snapshots
the counters only after that first call, so the assertion exposed only the
second call's identical +1 hit-path probe (`338` versus `337`), rather than two
probes.

There is no executed Buffer/typed-array registry call left on the tested
cached-hit path. The residual source call is now
`crates/perry-runtime/src/object/map_set_subclass.rs:81`, but it is reachable
only after `try_read_tracked_gc_header` misses at `:67`. A tracked
`GC_TYPE_OBJECT` returns directly from `instance_object_ptr` and never reaches
that side-table fallback.

## Complete byte-storage call audit on the cached plain-object `toString` path

This is a static enumeration, cross-checked by the exact counter test.

1. **Bare-receiver canonicalization.** The receiver is already NaN-boxed, so
   `canonicalize_bare_gc_receiver` returns at
   `object/native_call_method/bare_receiver.rs:114-115`. Its headerless-owner
   probes (`is_registered_buffer`, `is_any_array_buffer`,
   `is_uint8array_buffer`, and `lookup_typed_array_kind`) at `:175-178` are not
   reached. Consequently neither `is_registered_buffer_slow` nor
   `is_uint8array_buffer_slow` can be reached from this funnel.

2. **Site-cache revalidation.** `classify_native_receiver` reads the
   allocator-tracked header and `ObjectHeader.class_id` at
   `object/native_call_method.rs:176-182`; `receiver_kind_cache_lookup` at
   `:183` revalidates all of `(site_id, GC type, class id)`. The cache hit
   returns `NativeReceiverClass::GcObject(class_id)`. The classifier's
   `is_registered_buffer` and `lookup_typed_array_kind` calls at `:199` and the
   following typed-array fallback execute only after a tracked-header miss, so
   the cache revalidation itself performs no registry probe.

3. **Class-vtable fast guard.** `class_vtable_fast_guard_classified` consumes
   the already-computed receiver class at `native_call_method.rs:282-320`; it
   does not reclassify. This fixture has class id zero, so it returns at
   `:318-319`, before the own-key scan. There is no Buffer or typed-array call
   in this guard.

4. **Early dispatch and handle checks.** The only typed-array registry call in
   `dispatch_primitive` is the raw, untagged-pointer arm at
   `object/native_call_method/primitive_methods.rs:907-926`; a NaN-boxed object
   cannot enter it. `dispatch_handle` at
   `object/native_call_method/handle_methods.rs:55-171` now selects its Buffer
   and typed-array arms solely from `NativeReceiverClass`; `GcObject(0)` selects
   neither. `dispatch_raw_pointer` likewise rejects a NaN-boxed receiver at
   `object/native_call_method/collection_methods.rs:350`. The Buffer calls in
   `common_methods.rs:143` and `:268` belong only to the `hasOwnProperty` and
   `propertyIsEnumerable` match arms; the requested method is `toString`.

5. **Map/Set dispatch (the measured defect).** The boundary preserves class id
   in `GcObject(u32)`. At `native_call_method.rs:2166`,
   `may_dispatch_map_set` checks the tracked class ancestry first. For this
   receiver `is_map_set_subclass_class_id(0)` returns false at
   `object/map_set_subclass.rs:97-98`, so `dispatch_map_set` is not entered.
   Genuine `GC_TYPE_MAP`/`GC_TYPE_SET`, class ids descending from the reserved
   Map/Set ids, and legacy headerless pointers retain their respective paths.
   Inside the subclass unwrap, tracked headers are authoritative at
   `map_set_subclass.rs:67-75`; its Map/Set/Buffer/typed side-table sequence at
   `:79-82` is now only the tracked-header-miss fallback.

6. **Own-method lookup.** The object has one own key, `buffer`, so the method
   scan calls `js_array_get` for its internal keys array at
   `native_call_method.rs:2416`. `js_array_get_f64` classifies the resolved
   internal array from its tracked header at `array/indexing.rs:584-600`.
   Its `lookup_typed_array_kind` and `is_registered_buffer` expressions at
   `:591` and `:600` require a tracked-header miss; `GC_TYPE_ARRAY` therefore
   executes neither. The spelling `buffer` is only compared with `toString`;
   it cannot change the storage class.

7. **Prototype method lookup.** `resolve_inherited_field` at
   `native_call_method.rs:2459` first checks the recorded per-object prototype;
   the fixture has none. `ordinary_object_prototype_property_value` at `:2467`
   then obtains the cached Object.prototype address and reads `toString` through
   `js_object_get_field_by_name` (`field_get_set/accessors.rs:203-212` and
   `:219-272`). A field IC hit bypasses the object tail. On an IC miss, the tail
   reads Object.prototype's tracked header at
   `field_get_set/get_field_by_name_tail.rs:270-271`; its Buffer fallback at
   `:282` and typed-array lookup at `:453` both require no tracked header and do
   not run for Object.prototype's `GC_TYPE_OBJECT` allocation. No
   `is_array_buffer`, `is_shared_array_buffer`, or Uint8Array probe is reached
   because the enclosing `is_buffer` branch is false.

8. **Built-in thunk and brand derivation.** The resolved built-in is
   `object_prototype_to_string_thunk` at
   `object/global_this/array_error.rs:175-185`, which calls
   `js_object_to_string`. Brand derivation reads the receiver's tracked GC type
   at `object/to_string_tag.rs:158-160`. Its Buffer fallback at `:205-208` and
   typed-array lookup at `:242-247` are restricted to `tracked_type.is_none()`.
   Because the plain receiver is `GC_TYPE_OBJECT`, `is_array_buffer` and
   `is_shared_array_buffer` at `:212-215` are also unreachable inside the false
   Buffer branch. `@@toStringTag` lookup uses symbol property/prototype tables,
   not any byte-storage registry. The final `[object Object]` construction at
   `:470-476` calls only `js_string_from_bytes` and has no registry probe.

The private `lookup_registered_typed_array_kind` slow lookup is reachable only
through `lookup_typed_array_kind`; every such call named above is skipped for
the tracked plain receiver. Likewise the Buffer and Uint8Array slow functions
are reachable only through their public admission functions, none of which
executes on this path.

## Change

- `NativeReceiverClass` now retains `ObjectHeader.class_id` as
  `GcObject(u32)`, including on a site-cache hit.
- `dispatch_map_set` is called only for real Map/Set GC types, class ids whose
  parent chain reaches Map/Set, or the residual headerless pointer class.
  Tracked subclass instances may unwrap their hidden backing; a tracked object
  with no backing cannot fall into raw collection registries.
- `instance_object_ptr` now classifies allocator-tracked storage first and
  consults the legacy Map/Set/Buffer/typed registries only after a tracked-header
  miss.
- The neighboring hot `set_prototype_of -> get_prototype_of -> is_node_buffer`
  route received the same treatment at `buffer/exotic_view.rs:76-84`: a tracked
  header answers Buffer identity, and `is_registered_buffer` is only the
  headerless fallback. A tracked plain object short-circuits before the
  ArrayBuffer/DataView/Uint8Array subtype checks.
- The assertion and fixture were not changed.

## Verification

Final-tree checks:

```text
cargo test -j4 -p perry-runtime --release --lib \
  object::native_call_method::receiver_class_tests::cached_plain_object_receiver_probes_zero_buffer_registries \
  -- --exact --test-threads=1
  PASS: 1 passed; 0 failed; 3267 filtered out

direct rebuilt test binary: object::native_call_method::receiver_class_tests
  PASS: 6 passed; 0 failed
direct rebuilt test binary: object::map_set_subclass::tests
  PASS: 4 passed; 0 failed
direct rebuilt test binary: buffer::exotic_view_tests
  PASS: 17 passed; 0 failed

cargo build -j4 --release -p perry-runtime --features wasm-host
  PASS

rustfmt --edition 2021 (four changed Rust files)
  PASS
git diff --check
  PASS
```

All Cargo invocations above ran through
`/Users/amlug/projects/perry/secret-tests/cc-perf-campaign/measure_lock.sh --build`.
Disk checks showed 25 GB free before the final exact test and 15 GB before the
final archive feature build.

The full single-threaded runtime suite was attempted before the last
compatibility-preserving adjustment to the headerless fallback. It exited 101
with SIGSEGV in
`async_hooks::test_support::tests::native_async_resource_accepts_string_and_symbol_expandos`,
before reaching the receiver-class tests. That async-hooks test passed 1/1 when
rerun exactly, so the failure is order/global-state sensitive rather than a
failure of the changed path. The final-tree exact and focused tests listed
above passed.

A preliminary `cargo build -j4 --release -p perry` passed before that final
fallback adjustment. Immediately before a final-tree rerun, the mandatory disk
check was:

```text
Filesystem     1G-blocks Used Available Capacity
/dev/disk3s1s1       926   15        11    58%
```

Because 11 GB is below the binding 12 GB floor, no further Cargo command was
invoked and no wait for disk recovery was performed. Thus the full suite and
top-level `perry` build did not complete as final-tree gates.

`python3 scripts/addr_class_inventory.py` was run and failed only on existing
branch findings outside this patch: `object/to_string_tag.rs` has 11 frozen
handle-floor sites versus 10 allowed, and `intl/segments_view.rs:101` has an
allowlist-unregistered GcHeader cast. `scripts/check_file_size.sh` likewise
reported the existing 2004-line
`object/field_get_set/get_field_by_name_tail.rs`. This patch changes none of
those files.
