### Fixed

WebAssembly modules compiled with `--enable-wasm-runtime` can now share imported
functions, tables, memories, and mutable globals across Emscripten main/side
module instances, including imports resolved through a JavaScript `Proxy`.
`WebAssembly.Table`, `WebAssembly.Global`, and host-backed `WebAssembly.Memory`
constructors now expose linkable resources, wasm-bindgen externrefs cross import
callbacks intact, i64 values preserve their exact `BigInt` bits, and byte/file
dynamic imports with `{ with: { type: "wasm" | "file" } }` embed their assets.

Set `PERRY_WASM_TRACE=1` or `PERRY_WASM_DIAGNOSTICS=1` to report module byte
sizes plus import/export names while diagnosing standalone wasm loading.
