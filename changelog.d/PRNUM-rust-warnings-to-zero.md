### Fixed

- **Rust warnings: 492 → 0, and a CI gate so they stay there.** A clean
  `cargo check --workspace --all-targets` on `main` emitted 492 warnings
  (150 `unused_unsafe`, 121 `dead_code`, 96 `unused_imports`, 31
  `unreachable_patterns`, and eleven smaller families). Nothing in CI gated
  them: the `lint` job runs only `cargo fmt --check`, and the `clippy` job
  exits non-zero only on deny-level lints. Four scopes now report zero —
  `--workspace --all-targets`, `-p perry --bins`, `-p perry-runtime
  --no-default-features`, and the four-crate runtime/stdlib build — and a new
  `rustc-warnings` job runs `cargo check` with `-D warnings` over the product
  and host-compatible scopes.

  Three findings the sweep turned up, each fixed rather than silenced:

  - `js_node_http_res_write` and `js_node_http_res_end` were declared in an
    `extern "C"` block in `perry-ext-http-server` for symbols that crate
    defines itself. A local declaration of a symbol you also define is never
    checked against the definition — the defect class that shipped an ABI
    mismatch in #6646. Both signatures matched; the declarations are gone and
    the remaining ~62 in that block are tracked separately.
  - `duplex_allow_half_open_defaults_true_and_honors_false_option` had no
    `#[test]` attribute, so it had never run.
  - `test_seed_class_parent_closure_root` existed twice, both writing the same
    `CLASS_PARENT_CLOSURES` static.

  Eleven warnings appeared only under the reduced feature set `perry` selects
  (`default-features = false` on perry-runtime), where regex-engine,
  diagnostics and temporal are off. Those items are gated at the item, not
  suppressed. The cross-host UI crates (ios/tvos/watchos/visionos/android/
  windows/gtk4) cannot be checked from a macOS or Linux host and are untouched.
