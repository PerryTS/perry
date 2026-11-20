### runtime: keep wasm-bindgen memory and table exports live

WebAssembly exports now synchronize writes through `memory.buffer`, preserve
numeric multi-value results, coerce numeric arguments against the Wasm
signature, and expose `externref` table `get`/`set`/`grow`. This enables the
runtime state patterns used by file-packaged wasm-bindgen Node modules.
