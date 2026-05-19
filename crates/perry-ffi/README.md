# perry-ffi

Stable FFI surface for native bindings packages that wrap Rust crates for [Perry](https://github.com/PerryTS/perry), the TypeScript-to-native compiler.

This crate exists so wrapper authors (`perry-ext-*` in-tree, `@perryts/storekit` and other npm-distributed packages out-of-tree) don't depend on `perry-runtime` internals directly. Wrappers depend on this API-stable surface; `perry-runtime` field offsets, NaN-boxing details, and allocator hooks stay free to evolve underneath.

## Usage

In a wrapper crate's `Cargo.toml`:

```toml
[dependencies]
perry-ffi = "0.5"
```

In the wrapper's `package.json`:

```json
{
  "perry": {
    "nativeLibrary": {
      "abiVersion": "0.5",
      "functions": [
        { "name": "js_yourpkg_action", "params": ["string"], "returns": "promise" }
      ],
      "targets": { /* per-target build config */ }
    }
  }
}
```

The Perry compiler refuses to load a wrapper whose declared `abiVersion` is incompatible with the bundled `perry-ffi` version (#466 Phase 2).

## Features

- `runtime-link` (default OFF) — opt-in only for in-tree extension crates that need access to `perry-runtime`'s in-process types. External wrappers should leave this OFF: Perry's compiler driver statically links `libperry_runtime.a` into the final binary, so the runtime symbols are present at link time without a Cargo dependency edge.

## Versioning

The crate ships its own semver, currently tracking Perry's minor version (`0.5.x`). Any backwards-incompatible change to a function exported here bumps the perry-ffi major version, regardless of what `perry-runtime` does internally.

## Surface

See [`lib.rs`](src/lib.rs) for the full list. Today's surface covers what the smallest stdlib wrappers (`dotenv`, `nanoid`, `uuid`, `slugify`) need: allocate a JS string, read a JS string back, register a GC root scanner, hand objects/arrays/closures in and out of FFI calls.
