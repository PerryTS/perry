`perry compile --target wasip1` produces a WebAssembly core module for
WASI preview 1 (part of #13259). It imports only `wasi_snapshot_preview1`, so
it runs on runtimes without the component model, such as wasmtime started with
`-W exceptions=y`. `--target wasi` (preview 2, a component) is unchanged. The
two targets share the whole compile and link pipeline; `wasip1` links with
`--target=wasm32-wasip1`, exports no `cabi_realloc` and does no component
wrapping. `--output-type staticlib` is preview 2 only and reports that for
`wasip1`.

`scripts/wasi_run.py` and `scripts/wasi_image_selftest.py` take
`--flavour p1|p2` (default `p2`). The required `wasi-execution` job runs the
smoke, acceptance (`scripts/wasi_acceptance/p1/`) and gap probes on `wasip1`,
checks every linked module imports only preview 1, and runs the `link::wasi`
unit tests with the `target-wasi` feature.
