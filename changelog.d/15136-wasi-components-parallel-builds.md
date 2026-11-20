The `wasi-components` CI job runs its independent builds in parallel: the
instrumented runtime graph beside the main `perry-dev` build, and the wasm32
guest runtime archive beside the native component suites.
