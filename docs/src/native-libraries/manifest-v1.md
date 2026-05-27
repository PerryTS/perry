# `perry.nativeLibrary` manifest — spec v1

> **New here?** Start with [Native Bindings — Overview](overview.md)
> for the architectural picture and the
> [Authoring Guide](authoring-guide.md) for a step-by-step that uses
> this manifest. This page is reference-grade detail.

This page is the authoritative spec for the `perry.nativeLibrary`
field a native-bindings package declares in its `package.json`. The
Perry compiler reads this manifest at resolve time and uses it to:

1. Decide whether the import is "native" (calls into a Rust
   `staticlib`) vs. plain TypeScript / JavaScript.
2. Map TypeScript-side function calls onto the right
   `extern "C"` symbol with the right calling convention.
3. Pull the right `.a` archive into the link line, with the right
   frameworks / system libs / pkg-config dependencies for the
   user's compile target.

A companion JSON schema lives at
[`docs/api/manifest.schema.json`](../../api/manifest.schema.json) for
editor validation.

## Versioning

The schema is versioned via the `abiVersion` field. Every wrapper
declares which `perry-ffi` ABI it was built against:

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

The `perry` binary refuses to load a wrapper whose declared
`abiVersion` doesn't satisfy the bundled `perry-ffi`'s semver range.

**Transitional rule for the v0.5.x cycle**: missing `abiVersion`
is allowed but emits a warning naming the package and pointing at
this spec. From v0.6.0 onwards it becomes a hard error.

See [`docs/src/native-libraries/abi.md`](abi.md) for what the v0.5
ABI surface actually contains.

## Top-level shape

```jsonc
{
  "perry": {
    "nativeLibrary": {
      // Required from v0.6.0; warning-only in v0.5.x.
      "abiVersion": "0.5",

      // FFI function declarations — what TypeScript-side
      // call sites bind to. See "Functions" below.
      "functions": [
        { "name": "js_my_thing", "params": ["string"], "returns": "string" }
      ],

      // Per-target build configuration. Optional; if omitted, no
      // crate is built and the wrapper is purely a `.d.ts`-style
      // declaration of pre-built symbols (rare).
      "targets": {
        "macos":     { "...": "..." },
        "ios":       { "...": "..." },
        "linux":     { "...": "..." },
        "windows":   { "...": "..." },
        "android":   { "...": "..." },
        "web":       { "...": "..." },
        "harmonyos": { "...": "..." },
        "tvos":      { "...": "..." },
        "watchos":   { "...": "..." },
        "visionos":  { "...": "..." }
      }
    }
  }
}
```

## `abiVersion`

Semver string (e.g. `"0.5"`, `"0.5.3"`, `"^0.5"`).

The compiler interprets this as a range. The range must include the
bundled `perry-ffi`'s exact version. A wrapper declaring `"0.5"`
loads under any `0.5.x` Perry; one declaring `"0.5.3"` loads only
when the runtime is exactly `0.5.3`.

When the runtime fails the range check, compilation aborts with:

```
error: native library `<package>` declares perry-ffi ABI "0.5"
         but this Perry build ships perry-ffi 0.6.1.
       Update the package or use an older Perry release.
```

## `functions`

Array of function declarations. Each entry binds a TypeScript-visible
name to an `extern "C"` symbol exported by the wrapper's staticlib.

| Field    | Type            | Required | Notes                                         |
|----------|-----------------|----------|-----------------------------------------------|
| `name`   | string          | yes      | Symbol name (Perry prepends an underscore on macOS). |
| `params` | ABI descriptor[] | yes     | Parameter ABI descriptors — see "Param types" below. |
| `returns`| ABI descriptor   | yes     | Return ABI descriptor — see "Return types" below. |

ABI descriptors describe the native calling convention, not the
TypeScript type system. Perry keeps three layers separate:

- JS-visible values (`number`, `string`, opaque handles, promises)
- native ABI descriptors in the manifest (`f32`, `usize`, `buffer+len`)
- lowered LLVM/C ABI slots (`double`, `i64`, `ptr`, etc.)

Existing string spellings remain valid. The canonical descriptor
vocabulary is:

```text
jsvalue, string, bool, i32, i64, i64_str, u32, u64, usize,
f32, f64, number, ptr, buffer_len, buffer+len, handle<T>,
promise<T>, void
```

`number` is a compatibility alias for `f64`; `js_value` and `boolean`
are compatibility aliases for `jsvalue` and `bool`. Bare `handle` is
the same as an untyped `handle<T>`. Bare `promise` is the same as
`promise<jsvalue>`.

Descriptors with metadata may also use object form:

```json
{ "kind": "handle", "type": "MyThing" }
{
  "kind": "handle",
  "type": "MyThing",
  "ownership": "owned",
  "nullable": true,
  "thread": "creator",
  "finalizer": "my_thing_free",
  "debugName": "MyThing"
}
{ "kind": "promise", "result": "jsvalue" }
{ "kind": "buffer+len" }
```

Structured handles are GC-managed Perry native handle objects on the
JavaScript side. They are opaque and branded; user code cannot forge a
valid handle by passing a number or ordinary object. Use `"ptr"` only
when you intentionally want the raw pointer payload escape hatch.

Handle fields:

| Field | Values | Default | Notes |
|---|---|---|---|
| `type` | string | untyped | Branded handle type. Legacy `"handle<T>"` maps here. |
| `ownership` | `"borrowed"` / `"owned"` | `"borrowed"` | Owned return handles may run a native finalizer. Params may not declare finalizers. |
| `nullable` | boolean | `false` | Nullable handles may wrap a null resource pointer and unwrap to `0`. Non-null descriptors reject null handles. |
| `thread` | `"any"` / `"main"` / `"creator"` | `"any"` | Runtime validation rejects use from the wrong thread. |
| `finalizer` | symbol string | none | Valid only on owned return handles. The symbol must have `void(ptr, ptr)` ABI and must not call Perry JS APIs during GC. |
| `debugName` | string | `type` or `"handle"` | Stored inline for diagnostics. |

### Param types

| Manifest descriptor | Maps to Rust signature | TypeScript callsite view |
|---|---|---|
| `"jsvalue"` | `f64` | raw Perry NaN-boxed value |
| `"string"` | `*const StringHeader` | `string` |
| `"bool"` | `i32` truthy flag | `boolean` |
| `"i32"` | `i32` | `number` truncated to signed 32-bit |
| `"i64"` | `i64` | `number` converted to signed 64-bit |
| `"u32"` | `u32` | `number` converted to unsigned 32-bit |
| `"u64"` | `u64` | `number` converted to unsigned 64-bit |
| `"usize"` | `usize` | `number` converted to pointer-sized unsigned integer |
| `"f32"` | `f32` | `number` narrowed to 32-bit float |
| `"f64"` / `"number"` | `f64` | `number` |
| `"ptr"` | `i64` raw boxed pointer payload | raw pointer escape hatch |
| `"buffer_len"` | `u32` byte length | `number` |
| `"buffer+len"` | `(*const u8, usize)` | one Buffer/Uint8Array-shaped argument |
| `"handle"` / `"handle<T>"` | `i64` unwrapped resource pointer | opaque native handle |
| `"promise"` / `"promise<T>"` | `i64` promise handle | `Promise` handle metadata |

### Return types

| Manifest descriptor | Rust signature | TypeScript view |
|---|---|---|
| `"jsvalue"` | `-> f64` | raw Perry NaN-boxed value |
| `"string"` | `-> *const u8` *(see note)* | `string` |
| `"ptr"` | `-> *const u8` *(see note)* | `string` legacy pointer return |
| `"i64_str"` | `-> i64` | `string` (the `i64` is a `*StringHeader`) |
| `"bool"` | `-> i32` | `boolean` |
| `"i32"` | `-> i32` | `number` |
| `"i64"` | `-> i64` | `number` |
| `"u32"` | `-> u32` | `number` |
| `"u64"` | `-> u64` | `number` |
| `"usize"` | `-> usize` | `number` |
| `"f32"` | `-> f32` | `number` via explicit `f32 -> f64` materialization |
| `"f64"` / `"number"` | `-> f64` | `number` |
| `"buffer_len"` | `-> u32` | `number` |
| `"handle"` / `"handle<T>"` | `-> i64` resource pointer | opaque native handle object |
| `"promise"` / `"promise<T>"` | `-> i64` | JavaScript `Promise` |
| `"void"` | `-> ()` | `undefined` |

> Note on `"string"` vs. `"i64_str"`: both produce a string on the
> TypeScript side, but they differ in how Rust returns the pointer.
> Use `"string"` / `"ptr"` when your `extern "C" fn` is declared
> `-> *const u8` (or `*const StringHeader`); use `"i64_str"` when
> it's `-> i64` and the value happens to be a `StringHeader` address
> (closes [#222]).

`"void"` is valid only as a return descriptor. `"buffer+len"` is
valid only as a parameter descriptor because it expands one
JavaScript argument into two native ABI slots.

Native-only numeric descriptors (`f32`, `u32`, `u64`, `usize`,
`buffer_len`) render as TypeScript `number`. Handles remain opaque
GC-managed values, even though native functions still receive and
return raw `i64` resource pointers at the ABI boundary.
Promises remain JavaScript promises; the optional `promise<T>` result
metadata is currently recorded in compiler proof artifacts rather
than changing the runtime ABI.

## `targets.<target>`

Per-target build configuration. The `<target>` key is one of:
`macos`, `ios`, `linux`, `windows`, `android`, `web`, `harmonyos`,
`tvos`, `watchos`, `visionos`. Simulator variants use the same key
as their device counterpart (`ios` covers both `ios-simulator` and
`ios`).

| Field           | Type             | Required | Notes |
|-----------------|------------------|----------|-------|
| `crate`         | path string      | yes\*    | Path (relative to package.json) to the Cargo crate that produces the staticlib. Required when `prebuilt` is absent. |
| `lib`           | string           | yes\*    | Library name (without the `lib` prefix or `.a` extension). Required when `prebuilt` is absent. |
| `frameworks`    | array of string  | no       | Apple-only — system frameworks to pass to `clang -framework` (resolved from the SDK's `System/Library/Frameworks`). |
| `optionalFrameworks` | array of string | no  | Apple-only — vendored third-party frameworks linked **only** when `frameworksEnv` resolves to a directory containing them. `-framework <name>` per entry. Static frameworks only (see below). Snake_case `optional_frameworks` also accepted. |
| `frameworksEnv` | string           | no       | Name of an env var that points at the directory holding `optionalFrameworks`. When set + the path is a directory, `-F <dir>` is added to the link line; when unset, the optional frameworks are skipped silently. Snake_case `frameworks_env` also accepted. |
| `libs`          | array of string  | no       | System libraries to pass to the linker (`-lcurl`, etc.). |
| `libDirs`       | array of paths   | no       | Extra linker search paths. Emitted before `libs` as `-L<dir>` (or `/LIBPATH:<dir>` on Windows MSVC). Relative entries resolve against `package.json`. |
| `pkgConfig`     | array of string  | no       | pkg-config package names. The compiler runs `pkg-config --libs` and forwards the output. |
| `swift_sources` | array of paths   | no       | Swift sources to compile via `swiftc` and link in. Used by SwiftUI wrappers. |
| `metal_sources` | array of paths   | no       | Metal shader sources to compile via `xcrun metal` into `<app>.app/default.metallib`. |
| `prebuilt`      | path string      | no       | Path (relative to package.json) to a pre-built `.a` archive. When present, Perry uses this instead of running `cargo build`. |

When both `prebuilt` and `crate`/`lib` are absent for the user's
compile target, the wrapper is silently skipped on that target —
useful for platform-specific bindings that only exist on macOS, etc.

### Vendored frameworks (`optionalFrameworks` + `frameworksEnv`)

Some Apple SDKs can't be redistributed through npm (licensing) or
are too large to vendor — GoogleSignIn is the canonical example. For
these, the wrapper declares the SDK's framework name(s) in
`optionalFrameworks` and the name of an environment variable in
`frameworksEnv`. The app developer builds/downloads the framework
locally, points the env var at the directory holding it, and Perry's
linker adds `-F <dir>` plus `-framework <name>` for each entry.

```json
"targets": {
  "ios": {
    "crate": "crate-ios",
    "lib": "perry_google_auth",
    "optionalFrameworks": ["GoogleSignIn"],
    "frameworksEnv": "PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR"
  }
}
```

```bash
PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR=/path/to/Frameworks \
  perry compile app.ts --target ios
```

When the env var is **unset** (or points at a non-directory), the
optional frameworks are skipped silently. This pairs with a Swift
bridge guarded by `#if canImport(GoogleSignIn)`: the no-SDK fallback
compiles and the binary still links, returning a runtime
"framework not linked" result instead of failing with undefined
symbols. The same `build.rs` opt-in (`-F $DIR` to `swiftc`) must
gate the bridge's compile so both halves agree.

**Contract — static frameworks only.** `-framework` links the
archive directly; Perry does **not** embed the `.framework` into
`<app>.app/Frameworks/` or add an `@executable_path/Frameworks`
rpath. A dynamic framework would link but fail to load at runtime.
Vendor a statically-linked `.framework` (or a `.xcframework` slice
containing a static Mach-O). Embedding dynamic frameworks +
resource bundles is tracked as future work (#1304).

## Resolution

1. The user writes `import { foo } from "@perry/iroh"`.
2. Perry resolves `@perry/iroh` against `node_modules/`. If a
   matching directory has a `perry.nativeLibrary` manifest in its
   `package.json`, **this file's spec applies** and the wrapper is
   used.
3. If `node_modules/<name>/` exists *without* a manifest, the import
   falls through to V8 (existing behavior — TypeScript / JavaScript
   package).
4. If no `node_modules` entry matches, Perry consults its
   built-in well-known bindings table (see #466 Phase 4) — the
   same spec applies to the bundled wrapper.
5. None of the above match → resolution error.

A wrapper installed in `node_modules` always beats the well-known
table — that's how users override a bundled binding with a fork or
a beta version.

## Reference example

Minimal — three FFI functions, two targets. Matches the
`perry-ext-dotenv` shape:

```json
{
  "name": "@perry/dotenv",
  "version": "0.5.0",
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        { "name": "js_dotenv_config",      "params": [],          "returns": "number" },
        { "name": "js_dotenv_config_path", "params": ["string"],  "returns": "number" },
        { "name": "js_dotenv_parse",       "params": ["string"],  "returns": "string" }
      ],
      "targets": {
        "macos":   { "crate": "native/macos",   "lib": "perry_ext_dotenv" },
        "linux":   { "crate": "native/linux",   "lib": "perry_ext_dotenv" }
      }
    }
  }
}
```

A larger reference is Bloom Engine's manifest (~230 functions,
6 targets, frameworks + metal_sources) in the `bloom` repo.

## Compatibility & migration

The manifest schema is itself versioned by `abiVersion`. The major
version of `perry-ffi` is the major version of this manifest spec —
they move in lockstep:

- **0.5.x** — current; `abiVersion` is recommended but optional.
- **0.6.0** — `abiVersion` becomes required; missing field is a
  hard resolution error.
- **1.0.0** — first stable release; backwards-compat guarantees
  begin.

Anything not documented on this page (custom keys, undocumented
`returns` values) is **unsupported** and may break between releases.
File a request under [#466] and we'll consider adding it to v1.

[#222]: https://github.com/PerryTS/perry/issues/222
[#466]: https://github.com/PerryTS/perry/issues/466
