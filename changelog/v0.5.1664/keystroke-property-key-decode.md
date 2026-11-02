### Performance

- **Property reads no longer UTF-8-validate ASCII keys, and no longer copy
  the key or take the async-resource registry lock for ordinary receivers.**
  The generic read ladder decodes the key `StringHeader` at several layers
  per miss (`js_object_get_field_ic_miss`, closure expando lookup, accessor
  and reflection probes, the typed-feedback class-field guards, async-
  resource dispatch); `core::str::from_utf8` on those decodes was 2 % of the
  claude-code keystroke profile, the guard's `String` copy was a `malloc`
  per guarded class-field access, and `async_resource_property` copied the
  key and locked the registry before asking whether any AsyncResource
  handle existed at all.

  - `crates/perry-runtime/src/string/mod.rs` — `header_str_checked`: a
    header whose `utf16_len == byte_len` is pure ASCII, so it is borrowed
    unchecked; anything else takes the `from_utf8` scan it always took
    (WTF-8 payloads still answer `None`). Used by `has_own_helpers`,
    `closure_dynamic_prop_by_key`, the accessor probes,
    `typedarray_props::string_header_str` and the typed-feedback guards
    (which now borrow instead of allocating a `String`; every consumer is a
    Rust-side table read, so the payload cannot move while borrowed).
  - `crates/perry-runtime/src/async_hooks.rs` — `is_async_resource_handle`
    answers from the atomic handle count before touching the mutex, and the
    IC-miss handler / `async_resource_property` ask it before decoding or
    copying the key.

  Test: `header_str_checked_matches_from_utf8_on_every_payload_class`
  (ASCII, non-ASCII scalar, lone surrogate, empty).
