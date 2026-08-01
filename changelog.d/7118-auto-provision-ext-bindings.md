`perry compile` now auto-detects, auto-builds, and auto-links every native ext
binding and the wasm host a program actually references — no manual
`cargo build -p …` prebuild, no `PERRY_FORCE_WELL_KNOWN`, and no dependence on a
module being outside `compilePackages`.

- Link: codegen now routes any emitted `js_<binding>_*` FFI (`js_ioredis_*`,
  `js_undici_*`, `js_node_forge_*`) to its well-known wrapper off provenance
  alone, so an AOT-compiled `iovalkey`/`undici`/`node-forge` (a
  `compilePackages` member that never appears in any import set) links its
  `perry-ext-*` staticlib instead of failing with
  `Undefined symbols: _js_ioredis_new`.
- Build: routed CPU-only ext staticlibs are auto-built from workspace source
  when missing (the shared-tokio ones were already rebuilt in the auto-optimize
  invocation), and `perry-wasm-host` is auto-built whenever the program uses
  `WebAssembly.*`, so `libperry_wasm_host.a not found` can no longer happen on a
  normal compile.
- Maintainer audit: propagate custom `CARGO_TARGET_DIR` wasm-host artifacts
  through symbol scanning, link manifests, and the final link, and reject
  incomplete HarmonyOS SDKs before configuring their cross-build environment.
