# Native-call receiver test fix

## Pushed implementation

- SHA: `e02fb408b20114670b8330a07eefe9cf2d059c4d`
- Remote verification:

  ```text
  e02fb408b20114670b8330a07eefe9cf2d059c4d	refs/heads/perf/native-call-receiver-class
  ```

## Root causes and changes

1. `intl::segments_view::view_mode_tests::view_regexp_pointer_is_validated_exactly_once_per_call`

   `js_segments_view_regexp_test` performs the required exported-boundary brand
   validation at `crates/perry-runtime/src/intl/segments_view.rs:391`. On the
   first match, `regexp_test_str_bounded` reaches `ensure_regex_compiled`, whose
   cold `build_and_install_programs` path repeated `is_valid_regex_ptr`. That
   made a cold exported call count two validations. The duplicate cold-builder
   check was removed at `crates/perry-runtime/src/regex/lazy.rs:224`; the
   boundary check remains authoritative and the assertion was not changed.

2. `object::native_call_method::receiver_class_tests::cached_plain_object_receiver_probes_zero_buffer_registries`

   The native-call boundary correctly classified the receiver from its tracked
   `GC_TYPE_OBJECT`, but later Object-brand and defensive header-safety helpers
   bypassed `NativeReceiverClass`. In particular,
   `typedarray::is_offheap_sidetable_alloc` unconditionally entered both byte
   storage registries, and `Object.prototype.toString` independently probed the
   Buffer and typed-array registries while determining the brand. Temporary
   counters reproduced 30 typed-array registry calls during the one measured
   cached call; the native-call diagnostic buckets stayed at zero, confirming
   the traffic bypassed the classified native-call sites. Buffer counter
   admission is allocator-window-dependent (perrymaster observed 37).

   `crates/perry-runtime/src/typedarray/mod.rs:535` now proves tracked GC
   storage first and consults side tables only after a tracked-header miss.
   `crates/perry-runtime/src/object/to_string_tag.rs:80` and `:158` similarly
   derive managed Buffer/TypedArray identity (and typed-array kind) from the GC
   type/payload, reserving registry fallback for headerless storage.

3. `object::native_call_method::receiver_class_tests::cached_site_revalidates_when_plain_receiver_becomes_buffer`

   The receiver cache did revalidate the replacement as `GC_TYPE_BUFFER`, but
   pre-dispatch brand/header-safety checks still called
   `is_offheap_sidetable_alloc`, which treated every Buffer as legacy raw
   storage and entered its side registry (seven admitted probes in
   perrymaster's run). The tracked-header-first guard at
   `crates/perry-runtime/src/typedarray/mod.rs:541` now rejects every managed
   allocation before either registry. Headerless external Buffer/SAB/native
   typed views retain the old registry fallback at line 544. The test assertion
   was not changed.

## Verification

The first permitted exact diagnostic run started with 16 GB free and used the
campaign build lock, release mode, `-j4`, a non-real `HOME`, and one test
thread. It ran before the receiver-storage fixes, with temporary caller
instrumentation, and reproduced the failure:

```text
test object::native_call_method::receiver_class_tests::cached_plain_object_receiver_probes_zero_buffer_registries ... FAILED
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 3267 filtered out; finished in 0.01s
```

After that compile, `df -g /` was below 12 GB. It was polled every 60 seconds
for the full 30-minute recovery window (05:54-06:24 CEST). The final poll was:

```text
/dev/disk3s1s1       926   15         0    96%  458726 7505000    6%   /
```

Per the task's disk rule, no further Cargo invocation was made. Post-fix test
verdicts are therefore:

```text
intl::segments_view::view_mode_tests::view_regexp_pointer_is_validated_exactly_once_per_call — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::cached_plain_object_receiver_probes_zero_buffer_registries — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::cached_site_revalidates_when_plain_receiver_becomes_buffer — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::buffer_uint8array_and_arraybuffer_keep_their_method_paths — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::external_buffer_and_sab_backed_view_keep_native_dispatch — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::cached_receiver_kind_does_not_hide_reassigned_prototype — NOT RUN (disk gate)
object::native_call_method::receiver_class_tests::primitive_receiver_tag_skips_byte_storage_registries — NOT RUN (disk gate)
intl::segments_view::view_mode_tests::view_cursor_brand_is_a_class_load_with_zero_registry_probes — NOT RUN (disk gate)
```

Non-Cargo checks completed:

```text
rustfmt --edition 2021 --check [three changed Rust files] — exit 0
git diff --check — exit 0
```

## Not verified

- Full `cargo test -p perry-runtime --release --lib -- --test-threads=1`
- `cargo build --release -p perry-runtime --features wasm-host`
- `cargo build --release -p perry`

All temporary tracing was removed. The recoverable in-worktree `target`
directory created by the diagnostic compile was deleted after the disk reached
0 GB so Git could create and push the required commit.
