### runtime: instantiate compiled WebAssembly modules synchronously

`new WebAssembly.Instance(module, imports)` now routes through Perry's wasmi
host and returns live exports. This supports the synchronous, file-packaged
constructor shape used by wasm-bindgen Node loaders for Perry's existing
numeric function/import subset.
