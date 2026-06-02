# `perry-ffi` — the stable ABI for native bindings

This page documents the contract between native bindings packages
(`perryts/mysql2-bindings`, `@perry/iroh`, `perry-ext-dotenv`, …) and
the Perry runtime they execute inside.

> **New here?** Start with [Native Bindings — Overview](overview.md)
> for the architectural picture and the
> [Authoring Guide](authoring-guide.md) for the step-by-step. This page
> is reference-grade detail.

It is intentionally short. The whole point of the contract is
*minimum surface area* — every helper added is a forever
commitment, and Perry's internals (string layout, NaN-boxing tags,
GC) are free to change underneath as long as this surface holds.

## Versioning

`perry-ffi` ships its own semver, currently tracking Perry's minor:
`perry-ffi = "0.5"` for Perry `0.5.x`.

**v0.5.x consumption (current)**: until the v0.6.0 type-source-of-truth
refactor lands, `perry-ffi` re-exports a handful of types from
`perry-runtime` (`StringHeader`, `ArrayHeader`, `ObjectHeader`,
`BigIntHeader`, `BufferHeader`, `ClosureHeader`, `Promise`). That
makes it un-publishable to crates.io as-is — `perry-runtime` is a
private dep and not something we want on crates.io. Wrappers
depend on `perry-ffi` via the git URL while we're in the v0.5.x
cycle:

```toml
[dependencies]
perry-ffi = { git = "https://github.com/PerryTS/perry", branch = "main" }
```

`PerryTS/tursodb-bindings` and `PerryTS/iroh-bindings` ship this
shape and `cargo build` against the live `main` branch.

**v0.6.0 plan (deferred)**: invert the type ownership so `perry-ffi`
becomes the source of truth — it defines `#[repr(C)]` versions of
the ABI types itself, exposes opaque pointers for `Promise` /
`ClosureHeader` (which have private state), and `perry-runtime`
imports the types from `perry-ffi`. At that point `perry-ffi` has
zero `perry-runtime` deps and can publish to crates.io as
`perry-ffi = "0.6"` — wrappers switch to `cargo add perry-ffi`.
Tracked under [#466 Phase 1] as a v0.6.0 followup; the v0.5.x
git-URL approach is supported and tested end-to-end in the meantime.

A wrapper's `package.json` declares the ABI it was built against:

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

The Perry compiler refuses to load a wrapper whose declared
`abiVersion` doesn't satisfy the bundled `perry-ffi`'s semver range
(strict enforcement lands under issue [#466 Phase 2]). Backwards-
incompatible changes to anything in this document bump perry-ffi's
*major* version — independent of `perry-runtime` semver.

## Surface (v0.5.x)

The current surface is *deliberately minimal* — just enough to port
the simplest stdlib wrappers (`dotenv`, `nanoid`, `uuid`, `slugify`).
It will grow as real wrappers demand it; we'd rather under-design
and add than commit to a helper we later regret.

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

pub use perry_runtime::StringHeader; // for `*mut StringHeader` in extern "C" sigs
```

`alloc_string` allocates a fresh string in the runtime's arena.
The handle is owned by the runtime — Perry's GC reclaims it once
no live references remain, including references held by JS code
your function returned the handle to.

`read_string` borrows the underlying UTF-8 bytes for the duration
of the FFI call. Returns `None` on a null handle or invalid UTF-8.

`StringHeader` is re-exported as the canonical type for `extern "C"`
return / parameter types — wrappers should write
`pub extern "C" fn js_my_module_thing() -> *mut perry_ffi::StringHeader`,
not import `StringHeader` from `perry-runtime` directly.

### What's NOT in v0.5

These will land as real wrappers force them, tracked under
[#466 Phase 1]'s "Open questions":

- Array allocation / read (`alloc_array`, `read_array`).
- Object field get / set.
- Closure invocation helpers.
- NaN-boxing constants (undefined / null / true / false).
- Async runtime sharing (`spawn_async`, `block_on`).
- BigInt allocation.

If your wrapper needs one of these today, add it to perry-ffi in
the same PR that ports the wrapper. Treat this document as the
review gate: any addition needs a one-line entry above and a
unit test in `crates/perry-ffi/src/lib.rs`.

## Reference example: `perry-ext-dotenv`

The smallest stdlib wrapper Perry ships is the acceptance test for
the surface above. Its full FFI surface is two functions:

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

Source: [`crates/perry-ext-dotenv/src/lib.rs`][src].

It depends only on `perry-ffi` and `serde_json`. Zero references to
`perry-runtime` internals. That's the bar for every wrapper that
moves out of `perry-stdlib` over the course of #466 Phase 5.

## Followup roadmap

- [#466 Phase 2] freezes the `perry.nativeLibrary` manifest spec and
  enforces `abiVersion` at resolve time.
- [#466 Phase 3] adds `perry native init/validate/prebuild` for
  scaffolding new wrapper packages.
- [#466 Phase 4] adds the well-known bindings table so `import
  'dotenv'` resolves to `perry-ext-dotenv` automatically — until it
  lands, `import 'dotenv'` continues to bind to the
  `perry-stdlib` copy.
- [#466 Phase 5] ports the rest of the wrappers in size order
  (`uuid`, `nanoid`, `slugify`, `bcrypt`, `argon2`, then `ws`, then
  the database batch).

[#466 Phase 1]: https://github.com/PerryTS/perry/issues/466
[#466 Phase 2]: https://github.com/PerryTS/perry/issues/466
[#466 Phase 3]: https://github.com/PerryTS/perry/issues/466
[#466 Phase 4]: https://github.com/PerryTS/perry/issues/466
[#466 Phase 5]: https://github.com/PerryTS/perry/issues/466
[src]: https://github.com/PerryTS/perry/blob/main/crates/perry-ext-dotenv/src/lib.rs
