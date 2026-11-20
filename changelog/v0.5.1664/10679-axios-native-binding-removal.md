Removed the axios native binding (`crates/perry-ext-axios`, `perry-stdlib/src/axios.rs`,
and their `js_axios_*` FFI surface) so `import axios from "axios"` resolves to the real
npm package instead of Perry's hand-written reimplementation. First package removed under
the owner's decision to delete native bindings rather than let them drift from upstream
behavior — the motivating case was `jsonwebtoken.verify` silently accepting forged tokens.

**Base branch note**: this PR must not merge before #10673 (the `agent-base`
namespace/`export =` fallback fix) — axios's `https-proxy-agent` dependency needs that
fix to compile at all.

**What was removed**: the `NATIVE_MODULES` entry and manifest method rows (`entries.rs`,
`entries/part_4.rs`), the `[bindings.axios]` block in `well_known_bindings.toml`, the
`perry-ext-axios` crate and its `Cargo.toml`/workspace-member registration, the
`crates/perry-stdlib/src/axios.rs` implementation and its `common/dispatch/property_dispatch.rs`
response-property dispatch arm, the codegen dispatch for axios's static HTTP methods and
response-property access (`lower_call/options/fetch.rs`), the `js_axios_*` FFI declarations
(`runtime_decls/stdlib_ffi/third_party.rs`) and their runtime/no-op-stub implementations
(`perry-runtime/src/closure/v8_stubs.rs`, `perry-ui-android/src/stdlib_stubs.rs`), the
HIR "axios.get/post/… returns a Response" local-instance tagging used only to route
`.status`/`.data`/`.statusText` through the now-deleted native dispatch (`local_natives.rs`,
`destructuring/var_decl/native_new.rs`, `lower/module_decl.rs`, `lower/stmt.rs`), the
`emit.rs` "callable default export" special-case that existed only for axios's `axios(config)`
shape, and the `workspace-architecture.json` entry for the deleted crate.

Also removed: the axios-only HIR test (`axios_response_property_lowering.rs`) and the
native-dispatch NaN-boxing regression test (`test_issue_340_axios_response_props.ts`,
plus its stale `known_failures.json` entry) — both tested internals of the deleted native
shim and have no analog against real-source axios. Swapped `unimplemented_api_check.rs`'s
`supported_module_with_unknown_member_is_rejected` regression witness from axios to
node-fetch (still a `NATIVE_MODULES` member) since it needs a live native module to
demonstrate the #513 invariant. Regenerated `docs/api/perry.d.ts`, `docs/src/api/reference.md`,
and `docs/src/native-libraries/governance.md`'s table; dropped axios's row/section from
`docs/native-libraries.md` and its line from the `perry native list` example in
`docs/src/cli/commands.md`. Updated `workspace-architecture.json`'s baseline counts
(`workspace_members` 83→82, `decision_counts.externalize` 33→32) for the removed crate.

**Acceptance test — what a plain axios import needs**: a plain `import axios from "axios"`
with **no `perry.compilePackages` entry for axios at all** compiles and runs correctly —
verified against a live `node:http` GET/POST round-trip (`tests/release/packages/axios-get`,
whose `package.json` already had no `compilePackages` block and needed no new one) and a
second from-scratch repro. Perry's default "compile npm package source when no native
binding claims the specifier" path picks up axios and its full transitive dependency graph
(agent-base, https-proxy-agent, follow-redirects, form-data, combined-stream, mime-types,
debug, and the rest — the same ~26 packages the earlier `perry.compilePackages`-forced
probe used) automatically; `perry compile` reports "130 module(s): 130 native, 0 JavaScript"
for them. **No `package.json` configuration beyond a plain `"axios"` dependency + `npm
install` is required** — the `perry.compilePackages` list from the original probe is not
needed post-removal.

Validated: `cargo test` green for `perry-api-manifest` (41), `perry-hir` (all suites, 0
failed), `perry-stdlib` (139, `RUST_TEST_THREADS=1`), `perry` (1136 lib tests + the 5
integration tests whose comments mention axios: `incoming_message_pipe`,
`issue_10662_namespace_export_equals_fallback`, `issue_5174_headers_http_pump_hang`,
`response_stream_body_pull` — none depend on axios functionally, all green); the full
`crates/perry/tests/*.rs` integration sweep was not run (unrelated to this change, and
each fixture takes ~3 min on the shared build host — CI's `e2e-scoped` only runs
integration tests named by the diff, which is none here). `run_lint_gates.sh
SKIP_COMPILE_GATES=1`: 76 of 77 script gates pass; the one red
("Public benchmark evidence freshness") is pre-existing on every PR. `cargo fmt --all --
--check` clean. Two pre-existing, unrelated breakages on the base branch (confirmed via
`git stash` A/B, not touched here): `perry-codegen`'s test target fails to compile
(`ImportedClass` missing a field in two unrelated test files) and `perry-runtime`'s
`--tests` build carries one pre-existing dead-code warning in `box.rs`.
