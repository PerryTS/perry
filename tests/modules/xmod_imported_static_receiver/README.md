# Cross-module imported static receiver regression

This fixture has no Claude Code source or Bun-specific dependency. An exported
accessor calls a method on an uppercase named import, and a third module imports
only that accessor. Perry represents the receiver in `StaticMethodCall.class_name`,
not as an expression child. Its import must remain available: current main's
#9023 fix keeps this class-bearing call in its original module. An alternative
inliner may propagate and rename the import, but must never drop the receiver.

Before the fix, the localized accessor loses the receiver binding and codegen
silently returns numeric `0`. The native test fails with `lost factory result`;
Node prints `PASS: transitive imported static receiver`.

Run the native regression with a compiler and matching runtime archives:

```sh
PERRY_BIN=/absolute/path/to/perry PERRY_RUNTIME_DIR=/absolute/path/to/runtime \
  node scripts/test-xmod-imported-static-receiver.mjs
```

The transform unit tests assert that the imported static call remains in its
source module, alongside upstream's transitive class-dependency coverage.
Run them with `cargo test -p perry-transform --lib cross_module`.

If using runtime archives built with `perry-runtime/wasm-host`, also set
`PERRY_TEST_WASM=1` and provide their matching `libperry_wasm_host.a`.
