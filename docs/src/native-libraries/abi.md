# `perry-ffi` — the stable ABI for native bindings

This page documents the contract between native bindings packages
(`@perryts/iroh`, `@perryts/tursodb`, `perry-ext-dotenv`, …) and
the Perry runtime they execute inside.

> **New here?** Start with [Native Bindings — Overview](overview.md)
> for the architectural picture and the
> [Authoring Guide](authoring-guide.md) for the step-by-step. This page
> is reference-grade detail.

`perry-ffi` is deliberately smaller and more stable than
`perry-runtime`. It owns the public ABI types and helpers that wrapper
crates use while leaving runtime internals—field offsets, allocator
hooks, and NaN-boxing implementation details—free to change.

## Versioning and dependency setup

`perry-ffi` ships its own semver, currently tracking Perry's minor:
the `0.5.x` ABI accompanies Perry `0.5.x`. The crate is not currently
available from crates.io: publication is blocked on publishing its optional
`perry-runtime` dependency and that dependency's workspace chain first.
External wrappers therefore use the repository dependency emitted by
`perry native init`:

```toml
[dependencies]
perry-ffi = { git = "https://github.com/PerryTS/perry", branch = "main" }
```

For a released wrapper, pin a tested Perry tag or commit instead of allowing
an unreviewed `main` update to change the build underneath the release.

The crate defines its own `#[repr(C)]` ABI types, including
`StringHeader`, `ArrayHeader`, `ObjectHeader`, `BigIntHeader`,
`BufferHeader`, `ClosureHeader`, `Promise`, and
`NativeAsyncCompletion`. Do not import those types from
`perry-runtime`.

The optional `runtime-link` feature is for wrapper tests that need the
Perry runtime's symbol implementations in their test binary. External
wrappers should normally leave it disabled: Perry links the runtime archive
when it builds the final application.

A wrapper's `package.json` declares the ABI range it was built against:

```json
{
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "...": "..."
    }
  }
}
```

Perry validates this range when it resolves the package. An invalid
range or a range that excludes Perry's bundled `perry-ffi` version is
a compilation error. During the `0.5.x` cycle only, omitting
`abiVersion` emits a warning and continues; from `0.6.0`, omission is
also an error. A backwards-incompatible change to this ABI requires a
major `perry-ffi` version change, independently of `perry-runtime`.

## Current surface (`0.5.x`)

The table below groups the main public APIs. The
[`perry-ffi` exports][ffi-src] and their Rust documentation are the
source of truth for exact signatures and safety requirements.

| Area | Public surface |
| --- | --- |
| Strings and bytes | `JsString`, `alloc_string`, `read_string`, `alloc_bytes`, `read_bytes` |
| JavaScript values | `JsValue`, its value constants and conversions, `alloc_object`, `alloc_null_proto_object`, `build_object_shape` |
| Arrays and objects | `js_array_alloc/get/length/push/set`, `js_object_alloc_with_shape/get_field/set_field`, `object_field_by_name` |
| Closures | `JsClosure::call0` through `call4`, `alloc_closure`, capture accessors, arity registration |
| Buffers and BigInts | `alloc_buffer`, `read_buffer_bytes`, `alloc_bigint_from_str`, `read_bigint_limbs` |
| Async work | `JsPromise`, `JsNativeAsyncCompletion`, `spawn_async`, `spawn_blocking`, `spawn_blocking_with_reactor`, `run_pending` |
| Native state and GC | The typed handle registry, mutable root scanners, and `TransientRootScope` |
| Errors and integration | Error/warning helpers, `json_stringify`, auxiliary event-pump hooks, and `RawNetVtable` registration |

### Strings

```rust
pub struct JsString(/* opaque */);

pub fn alloc_string(s: &str) -> JsString;
pub fn read_string(handle: JsString) -> Option<&'static str>;

impl JsString {
    pub unsafe fn from_raw(ptr: *mut StringHeader) -> Self;
    pub fn as_raw(self) -> *mut StringHeader;
    pub fn is_null(self) -> bool;
}
```

`alloc_string` copies UTF-8 into a new runtime-owned string.
`read_string` returns `None` for a null handle or invalid UTF-8. The
returned bytes are borrowed from the runtime arena; do not free them or
retain a raw pointer beyond the lifetime guaranteed by the calling
context.

Use `perry_ffi::StringHeader` in exported signatures:

```rust
pub extern "C" fn js_my_module_thing() -> *mut perry_ffi::StringHeader
```

### Async promise rejection

`JsPromise::reject_string(message)` copies the message and rejects with a real
JavaScript `Error` allocated on the runtime's main thread. Its `.message` and
`.stack` are available to ordinary handlers, and `instanceof Error` succeeds.
`JsPromise::reject(value)` remains the escape hatch for APIs that deliberately
reject with an arbitrary JavaScript value. Use `JsPromise::reject_with` to
construct a structured rejection value safely on the main thread.

### ABI boundaries and thread safety

- Use the exported constructors, accessors, and `JsValue` conversions.
  Do not depend on private runtime field offsets or hard-code pointer
  tags.
- JavaScript heap allocation is tied to Perry's main-thread arena. If
  worker-thread work must produce an object, array, or other complex
  `JsValue`, carry plain `Send` data back and construct the value with
  `JsPromise::resolve_with` on the main thread.
- Register native values that retain JavaScript references with the
  handle/root-scanner APIs. Use `TransientRootScope` for temporary
  values that must survive an allocation or collection point.
- Respect the `unsafe` contracts on raw pointer constructors and FFI
  entry points. `perry-ffi` stabilizes the interface; it cannot prove
  that a caller supplied a live pointer of the declared type.
- Keep `runtime-link` out of production wrapper dependencies unless
  the crate genuinely needs a Cargo-level `perry-runtime` link. It is
  normally enabled only in wrapper tests.

Any new ABI helper should be documented and covered by a focused test
in `crates/perry-ffi` in the same change.

## Reference example: `perry-ext-dotenv`

The smallest in-tree wrapper demonstrates string input and output:

```rust
use perry_ffi::{alloc_string, read_string, JsString, StringHeader};

#[no_mangle]
pub unsafe extern "C" fn js_dotenv_config_path(
    path_ptr: *const StringHeader,
) -> f64 {
    let handle = JsString::from_raw(path_ptr as *mut _);
    let path = read_string(handle).unwrap_or(".env");
    // … read file, set env vars, return 1.0 / 0.0 …
}

#[no_mangle]
pub unsafe extern "C" fn js_dotenv_parse(
    content_ptr: *const StringHeader,
) -> *mut StringHeader {
    let handle = JsString::from_raw(content_ptr as *mut _);
    let Some(content) = read_string(handle) else {
        return std::ptr::null_mut();
    };
    let parsed = parse_dotenv_content(content);
    let json = serde_json::to_string(&parsed).unwrap_or_else(|_| "{}".into());
    alloc_string(&json).as_raw()
}
```

Source: [`crates/perry-ext-dotenv/src/lib.rs`][dotenv-src]. The crate's
normal dependency is `perry-ffi`; its dev-dependency enables
`runtime-link` so the FFI round-trip test can link runtime symbols.

## Tooling and implementation status

The native-library work tracked by [#466] has landed:

- manifests declare their native functions and ABI version;
- incompatible declared ABI ranges are rejected during resolution;
- `perry native init`, `perry native validate`, and
  `perry native list` provide authoring and inspection workflows;
- the well-known bindings table routes supported package imports; and
- wrappers can depend on the repository's stable `perry-ffi` API instead of
  `perry-runtime` internals.

Issue #466 is retained as the historical design record. Open a new
issue for a missing ABI helper or a new native-library capability.

[#466]: https://github.com/PerryTS/perry/issues/466
[ffi-src]: https://github.com/PerryTS/perry/blob/main/crates/perry-ffi/src/lib.rs
[dotenv-src]: https://github.com/PerryTS/perry/blob/main/crates/perry-ext-dotenv/src/lib.rs
