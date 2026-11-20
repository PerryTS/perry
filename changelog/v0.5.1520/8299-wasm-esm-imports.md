### compile: instantiate static `.wasm` ESM imports (#5234)

Static WebAssembly imports now embed and instantiate the module during module
initialization, exposing function and memory exports through namespace, named,
and default ESM imports. Numeric function imports are routed back through the
JavaScript imports object, including circular wasm-bindgen-style glue modules.
