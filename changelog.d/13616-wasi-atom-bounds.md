Fix the WASI runtime build with the `wasm-image-selftest` feature (the
`wasi-execution` CI job). The oversized-file split moved `atom_bounds` into the
private `layout_desc::atoms` module, so the linked wasm image checks
(`wasm_program_image_checks.rs`) and the Windows atom-bounds test, both
children of `layout_desc`, no longer resolved it (E0425). `atom_bounds` is now
`pub(super)` and imported into `layout_desc` under exactly the cfgs of its two
consumers; native and normal WASI builds are unchanged.
