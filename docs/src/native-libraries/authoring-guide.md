# Authoring a native binding

Step-by-step guide to writing and publishing a Rust binding that
Perry programs can `import` like any npm package.

For the architectural picture this fits into, see
[Native bindings — overview](overview.md).

## Prerequisites

- A Rust crate you want to expose to TypeScript (e.g. `pdfium-render`,
  `image`, your own internal library).
- Rust toolchain installed.
- `perry` on your `PATH` (the [`perry native`](../cli/native.md)
  subcommand ships with the install).
- A GitHub account if you want the prebuild release-CI scaffold to
  Just Work.

## 1. Scaffold

```sh
perry native init my-bindings \
  --description "Native bindings for <upstream crate>" \
  --upstream-dep '<crate-name> = "<version>"' \
  --github-owner <your-handle>

cd my-bindings
```

This creates:

```
my-bindings/
├── Cargo.toml                           # perry-ffi dep + your upstream
├── src/
│   ├── lib.rs                           # one example #[no_mangle] fn
│   └── index.ts                         # TS surface user code imports
├── package.json                         # perry.nativeLibrary block
├── README.md
├── LICENSE                              # MIT, swap if needed
├── .gitignore
└── .github/workflows/release.yml        # multi-target prebuild on tag
```

## 2. Add bindings

Each TypeScript-visible function should forward to one `extern "C"` Rust export.

### `src/lib.rs`

The example template starts with one `js_<name>_hello` function.
Replace it with your bindings — one `#[no_mangle] pub extern "C" fn`
per TypeScript-visible call, using **only** types from `perry_ffi`:

```rust
use perry_ffi::{alloc_string, StringHeader};

/// `pdf.parse(buf) -> string` — extract text from a PDF buffer.
///
/// # Safety
///
/// `buf_ptr` must be null or valid for `buf_len` bytes. Perry passes
/// this pair from a `buffer+len` manifest parameter.
#[no_mangle]
pub unsafe extern "C" fn js_pdf_parse(buf_ptr: *const u8, buf_len: usize) -> *mut StringHeader {
    let bytes = if buf_ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(buf_ptr, buf_len)
    };
    match pdfium_render::Pdfium::default().load_pdf_from_byte_slice(bytes, None) {
        Ok(doc) => {
            let text = doc.pages().iter().map(|p| p.text().unwrap()).collect::<String>();
            alloc_string(&text).as_raw()
        }
        Err(_) => std::ptr::null_mut(),
    }
}
```

Key rules:
- **Don't `use perry_runtime::*`**. perry-runtime's internals (NaN-box
  tags, struct layouts) change between Perry releases. perry-ffi is
  the stable contract.
- **Use `unsafe extern "C"` for any function that takes pointer args**.
  `*const StringHeader` etc. require unsafe at the call site.
- **Document `# Safety` for unsafe fns** — at minimum say "the
  pointer must be null or a Perry-runtime `<Header>`".
- **Async returns `*mut Promise`**. Pattern: `JsPromise::new()` →
  `spawn_blocking(move || { tokio::runtime::Handle::current().block_on(async {...}); promise.resolve(...) })`
  → return `promise.as_raw()`.

### `src/index.ts`

Declare the FFI symbol from your manifest, then export the TypeScript
surface user code imports. Perry's FFI dispatch keys on the call-site
identifier, so the wrapper body must explicitly call the `js_*` symbol
listed in `perry.nativeLibrary.functions[]`.

```typescript
{{#include ../../examples/_fixtures/native-libraries/my-bindings/src/index.ts}}
```

### `package.json`

The `perry.nativeLibrary` block tells Perry's compiler about every
`extern "C"` export plus the build config. Schema details in
[`manifest-v1.md`](manifest-v1.md).

```json
{
  "name": "my-bindings",
  "version": "0.1.0",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        {
          "name": "js_pdf_parse",
          "params": [{ "kind": "buffer+len" }],
          "returns": "string"
        }
      ],
      "targets": {
        "macos":   { "cargo_features": [] },
        "linux":   { "cargo_features": [] },
        "windows": { "cargo_features": [] }
      }
    }
  }
}
```

Every entry in `functions[]` must:
- have a `name` matching exactly the symbol the staticlib exports
  (Perry's `perry native validate` verifies this for you)
- declare `params` and `returns` so codegen knows the calling convention

Manifest descriptors are native ABI descriptors, not TypeScript
surface types. Existing strings such as `"string"`, `"number"`,
`"i64"`, and `"void"` still work. Use the explicit vocabulary when
you need native precision or metadata: `"f32"`, `"u32"`, `"u64"`,
`"usize"`, `"buffer_len"`, `{ "kind": "buffer+len" }`,
`{ "kind": "handle", "type": "MyThing" }`, and
`{ "kind": "promise", "result": "jsvalue" }`.

Handles are opaque Perry native handle objects in JavaScript, not raw
numbers. Legacy `"handle"` and `"handle<T>"` descriptors still parse
as borrowed handles; structured descriptors can add ownership,
nullability, thread affinity, debug names, and an owned-return
finalizer:

```json
{
  "kind": "handle",
  "type": "MyThing",
  "ownership": "owned",
  "thread": "creator",
  "finalizer": "my_thing_free",
  "debugName": "MyThing"
}
```

The finalizer symbol must use `void(ptr, ptr)` ABI and must only free
native resources. It can run from GC finalization, so it must not call
Perry JS APIs or allocate JS values. Use `"ptr"` when an API
intentionally accepts a raw pointer payload instead of a managed
handle.

## 3. Verify

```sh
perry native validate
```

This runs `cargo build --release`, locates the resulting `.a`,
walks `nm -gP` over its symbols, and diffs against the manifest's
`functions[]`. The output flags two failure modes:

- **❌ declared function has NO matching symbol** — your manifest
  lists a function the staticlib doesn't export. Either you typo'd
  the name, or you forgot `#[no_mangle]`.
- **⚠ `js_*` symbol NOT in the manifest** — your staticlib exports
  a function user code can't reach. Either add it to `functions[]`,
  rename it (drop the `js_` prefix), or remove it.

A green run looks like:

```text
perry native validate
======================
  package:    my-bindings
  abiVersion: 0.5
  staticlib:  ./target/release/libperry_ext_my_bindings.a
  declared functions:           1
  exported `js_*` symbols:      1
  ✅ manifest matches the staticlib.
```

## 4. Test in a Perry program

In a separate directory:

```sh
mkdir test-app && cd test-app
perry init
bun add file:../my-bindings   # or any path your tooling supports
```

Add to your TS:

```typescript
{{#include ../../examples/_fixtures/native-libraries/my-bindings/test-app.ts}}
```

Then `perry compile main.ts -o main && ./main`.

## 5. Publish

### Tag a release

```sh
git tag v0.1.0
git push --tags
```

The scaffolded `.github/workflows/release.yml` builds prebuilt
staticlibs for x86_64 + aarch64 macOS/Linux + Windows on tag and
attaches them to the GitHub release. Add or remove targets in the
workflow's `matrix` block as needed.

### npm publish

```sh
npm publish
```

The scaffolded `package.json` includes the right `files: [...]` list
to bundle `src/` + `Cargo.toml` + the README. If you also vendor the
prebuilt artifacts in the npm tarball, add them to the `files` block.

### Two distribution models

There are two ways your users get the staticlib:

| Model | What ships in the npm tarball | Trade-off |
|---|---|---|
| **Vendor prebuilts** | `src/`, `Cargo.toml`, AND `prebuilt/<target>/lib<name>.a` for every target | Bigger npm tarball; install is fast (no compile); user doesn't need a Rust toolchain |
| **Source-only** | `src/`, `Cargo.toml`, no prebuilts | Tiny tarball; first `perry compile` runs `cargo build --release` (slow); user needs Rust |

Vendoring is the friendlier default for npm consumers. Source-only
makes sense if your matrix is too big for one tarball or if you're
publishing a private wrapper to a small audience.

The manifest's `targets.<target>.prebuilt` field tells Perry where to
find a prebuilt for the user's compile target:

```json
{
  "perry": {
    "nativeLibrary": {
      "targets": {
        "macos":   { "prebuilt": "./prebuilt/macos/libperry_ext_my_bindings.a" },
        "linux":   { "prebuilt": "./prebuilt/linux/libperry_ext_my_bindings.a" },
        "windows": { "prebuilt": "./prebuilt/windows/perry_ext_my_bindings.lib" }
      }
    }
  }
}
```

If the prebuilt path doesn't exist on disk at compile time, Perry
falls back to `cargo build --release`.

## 6. Update over time

- **A new perry-ffi feature lands**: bump your `Cargo.toml`'s
  `perry-ffi` version, rebuild prebuilts, tag a new release. Users
  `bun update` to pick it up. Perry's manifest spec stays at v1
  unless the schema changes.
- **A new Perry minor**: same — `perry-ffi`'s semver moves with
  Perry's minor. The git-URL consumption (v0.5.x) means rebuilding
  against `main` picks it up automatically.
- **Breaking change to the `js_*` surface you exported**: bump your
  package's major version (`1.0.0` → `2.0.0`). Users who pin a
  major aren't affected.

## Common patterns

### Async one-shot (HTTP request, DB query)

```rust
use perry_ffi::{alloc_string, spawn_blocking, JsPromise, JsValue, Promise};

#[no_mangle]
pub extern "C" fn js_my_fetch(url_ptr: *const StringHeader) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let url = unsafe { read_str(url_ptr) }.unwrap_or_default();

    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            reqwest::get(&url).await.and_then(|r| Ok(r.text())).await
        });
        match outcome {
            Ok(body) => promise.resolve(JsValue::from_string_ptr(alloc_string(&body).as_raw())),
            Err(e)   => promise.reject_string(&format!("fetch: {}", e)),
        }
    });
    raw
}
```

### Sync handle-based class

Use a `handle` descriptor for synchronous resource-style APIs. The
native function receives or returns the raw resource pointer as `i64`,
while TypeScript callers see only the opaque handle object.

```rust
use perry_ffi::{get_handle, register_handle, Handle};

pub struct MyThing { val: u64 }

#[no_mangle]
pub extern "C" fn js_my_thing_new() -> Handle {
    register_handle(MyThing { val: 0 })
}

#[no_mangle]
pub extern "C" fn js_my_thing_get(h: Handle) -> f64 {
    get_handle::<MyThing>(h).map(|t| t.val as f64).unwrap_or(0.0)
}
```

### Event listeners (`.on(event, cb)`)

```rust
use perry_ffi::{
    gc_register_root_scanner, get_handle_mut, iter_handles_of, register_handle,
    Handle, JsClosure, RawClosureHeader, StringHeader,
};

pub struct EventEmitter {
    listeners: Vec<i64>,  // closure pointers, kept alive by the GC scanner below
}

static SCANNER_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_scanner() {
    SCANNER_REGISTERED.call_once(|| {
        gc_register_root_scanner(|mark| {
            iter_handles_of::<EventEmitter, _>(|emitter| {
                for &cb in &emitter.listeners {
                    if cb != 0 {
                        let nan_boxed = f64::from_bits(0x7FFD_0000_0000_0000 | (cb as u64 & 0x0000_FFFF_FFFF_FFFF));
                        mark(nan_boxed);
                    }
                }
            });
        });
    });
}

#[no_mangle]
pub extern "C" fn js_emitter_on(h: Handle, cb: i64) -> Handle {
    ensure_scanner();
    if let Some(e) = get_handle_mut::<EventEmitter>(h) {
        e.listeners.push(cb);
    }
    h
}

#[no_mangle]
pub extern "C" fn js_emitter_emit(h: Handle, arg: f64) -> bool {
    if let Some(e) = get_handle_mut::<EventEmitter>(h) {
        for &cb in e.listeners.clone().iter() {
            let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
            let _ = unsafe { closure.call1(arg) };
        }
        true
    } else {
        false
    }
}
```

The GC scanner is **load-bearing**: without it, a malloc-triggered
GC between `.on(cb)` and `.emit()` will sweep the closure and the
next emit calls freed memory. Always register a scanner if your
handles store closure pointers.

## When to extend the perry-ffi surface

If your binding genuinely needs something perry-ffi doesn't expose,
file an issue against
[`PerryTS/perry`](https://github.com/PerryTS/perry/issues) describing:

- the binding you're writing,
- the perry-runtime function/type you'd otherwise reach into,
- why a higher-level perry-ffi entry would generalize.

The bar for adding to perry-ffi is high — every helper is a forever
commitment — but real wrappers driving real needs is exactly the
right input. The recent additions (BigInt + Buffer in v0.5.556,
JSON-stringify + event-pump in v0.5.567 followups) all came from
specific wrappers needing them.

Don't reach into `perry_runtime::*` directly to "unblock" your
wrapper today — it'll break the next time those internals change.

## See also

- [`overview.md`](overview.md) — the architectural picture.
- [`abi.md`](abi.md) — perry-ffi reference.
- [`manifest-v1.md`](manifest-v1.md) — the manifest schema in full.
- [`PerryTS/tursodb-bindings`](https://github.com/PerryTS/tursodb-bindings)
  and
  [`PerryTS/iroh-bindings`](https://github.com/PerryTS/iroh-bindings)
  for end-to-end real-world examples.
