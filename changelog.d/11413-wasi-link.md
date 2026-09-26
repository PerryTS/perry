**WASI: `perry compile --target wasi` links a runnable component (#11379, phase 4 of #11375).** Behind the off-by-default `target-wasi` feature; native output does not change.

- The driver links the program's wasm32 objects against `libperry_runtime.a` built for `wasm32-wasip2` with wasi-sdk (`crates/perry/src/commands/compile/link/wasi.rs`). Flags: `--export=cabi_realloc`; `--stack-first` with an 8 MiB stack; `--fatal-warnings`, because wasm-ld only warns on a call/callee signature mismatch and links a trapping stub. The output is `<stem>.wasm`, a WASI 0.2 component (`wasmtime run app.wasm`).
- wasm32 lowering:
  - closure bodies take the closure as `ptr` (`wasm32/closure_abi.rs`), and direct calls to them go through the ABI adapters;
  - `@main` becomes wasi-libc's `__main_void`/`__main_argc_argv`;
  - the NUL terminator inkwell appends to the object buffer is dropped (wasm-ld aborted on it with "malformed uleb128").
- Runtime on WASI:
  - closure dispatch calls every body with exactly its real parameter count, read from the engine with `ref.test` (`ffi/perry_wasi_sig.c`, `-mgc`), because a mismatched `call_indirect` traps;
  - a throw that a `try` would catch reports that catching is unsupported and exits (#11378);
  - >32-argument closure calls throw a `RangeError`;
  - the heap floor and handle band are adjusted for wasm32.
- `scripts/runtime_abi_check.py` expands simple local `macro_rules!`, so macro-defined runtime entry points resolve (71 → 6 unresolved; +65 wasm ABI table rows).
- New `scripts/wasi_toolchain.sh` (pinned, sha256-verified wasi-sdk 34 + wasmtime 48), `scripts/wasi_build_runtime.sh`, and `scripts/wasi_smoke.sh`. The smoke test runs six programs under wasmtime against Node's output. The non-required `wasi-codegen` job runs all three.
- Known gap filed from this work: #11412 (NaN-boxed values through pointer-typed runtime parameters lose their tag on wasm32).
