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
| `params` | array of string | yes      | Parameter ABI types — see "Param types" below. |
| `returns`| string          | yes      | Return ABI type — see "Return types" below.   |

### Param types

| Manifest value | Maps to Rust signature       | When TS callsite passes …            |
|----------------|------------------------------|--------------------------------------|
| `"string"`     | `*const StringHeader`        | a JS string                           |
| `"number"`     | `f64`                        | any JS number                         |
| `"i32"`        | `i32`                        | a number truncated to int             |
| `"i64"`        | `i64`                        | a BigInt or large number              |
| `"bool"`       | `bool`                       | a boolean                             |
| `"ptr"`        | `*const c_void`              | an opaque handle (advanced)           |

### Return types

| Manifest value | Rust signature              | TypeScript view                            |
|----------------|-----------------------------|--------------------------------------------|
| `"string"`     | `-> *const u8` *(see note)* | a string                                   |
| `"ptr"`        | `-> *const u8` *(see note)* | a string                                   |
| `"i64_str"`    | `-> i64`                    | a string (the `i64` is a `*StringHeader`)  |
| `"i64"`        | `-> i64`                    | a number                                   |
| `"number"`     | `-> f64`                    | a number                                   |
| `"void"`       | `-> ()`                     | `undefined`                                |
| anything else  | `-> f64`                    | a number                                   |

> Note on `"string"` vs. `"i64_str"`: both produce a string on the
> TypeScript side, but they differ in how Rust returns the pointer.
> Use `"string"` / `"ptr"` when your `extern "C" fn` is declared
> `-> *const u8` (or `*const StringHeader`); use `"i64_str"` when
> it's `-> i64` and the value happens to be a `StringHeader` address
> (closes [#222]).

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
| `frameworks`    | array of string  | no       | Apple-only — frameworks to pass to `clang -framework`. |
| `libs`          | array of string  | no       | System libraries to pass to the linker (`-lcurl`, etc.). |
| `libDirs`       | array of paths   | no       | Extra linker search paths. Emitted before `libs` as `-L<dir>` (or `/LIBPATH:<dir>` on Windows MSVC). Relative entries resolve against `package.json`. |
| `pkgConfig`     | array of string  | no       | pkg-config package names. The compiler runs `pkg-config --libs` and forwards the output. |
| `swift_sources` | array of paths   | no       | Swift sources to compile via `swiftc` and link in. Used by SwiftUI wrappers. |
| `metal_sources` | array of paths   | no       | Metal shader sources to compile via `xcrun metal` into `<app>.app/default.metallib`. |
| `prebuilt`      | path string      | no       | Path (relative to package.json) to a pre-built `.a` archive. When present, Perry uses this instead of running `cargo build`. |

When both `prebuilt` and `crate`/`lib` are absent for the user's
compile target, the wrapper is silently skipped on that target —
useful for platform-specific bindings that only exist on macOS, etc.

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
