### CI: two gates that could never pass build the `-static` wrappers now (#8224, #8225)

`perry-runtime`/`perry-stdlib` are rlib-only since #5422 — `libperry_*.a`
come from the `perry-runtime-static`/`perry-stdlib-static` wrapper crates.
Two dark-since-July gates never learned that:

* `compile-smoke`'s build step was a bare `cargo build --release`
  (default-members only), so `test_precompile_basic` and
  `test_issue_414_mysql_query_params` — which link the archives directly —
  failed with "Could not find libperry_runtime.a" on every run this job ever
  had under `continue-on-error` (#8224). The build now names the wrapper
  crates. Also skip-lists the three fixtures from #8224 that were never
  registered: `test_ui_drag_drop` / `test_ui_text_alignment` (Linux host has
  no `libperry_ui_gtk4.a` — existing ci-env family) and
  `test_jwt_sign_dynamic_alg` (`js_jwt_sign_dyn`, sibling of the already
  skip-listed `test_jose_signverify_roundtrip`).
* `scripts/native_abi_evidence_packet.sh` built `-p perry-runtime` and then
  audited an archive that build cannot emit — `check_runtime_symbols`
  reported "libperry_runtime.a does not exist" since the script's creation
  (#8225). Both build arrays now use `perry-runtime-static`.

The packet's `native_abi_contract` drift and its 7 failing compiler-output
workloads (#8225 items 2–3) are real content regressions and stay red.
