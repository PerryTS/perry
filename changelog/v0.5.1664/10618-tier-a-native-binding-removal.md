Removed the three Tier A native bindings that were safe to delete without
any compiler work: the bare-name `fetch` alias for `node-fetch`, and the
leftover in-tree accounting for `tursodb`/`iroh` (their actual crates and
`well_known_bindings.toml` entries had already moved out to
`@perryts/tursodb` / `@perryts/iroh` in v0.5.557 — this finishes the job).

`fetch` alias: dropped `[bindings.fetch]` from `well_known_bindings.toml`
and the `"fetch"` entry from `NATIVE_MODULES`. `node-fetch` itself, its
`perry-ext-fetch` crate, and the manifest entries backing the built-in Web
Fetch API's `Response`/`Headers`/`Request`/`Blob`/`FormData` dispatch tag
(a separate mechanism, confirmed via the pre-existing
`builtin_fetch_usage_does_not_synthesize_well_known_fetch` test) are
unaffected; `"fetch"` moved to the test-only `INTERNAL_MODULE_KEYS`
allowlist so `known_modules_consistent_with_manifest` still holds.

`tursodb`/`iroh`: removed their `NATIVE_MODULES` entries, manifest method
rows, `stdlib_features.rs` feature-gate arms, and skip-list entries in the
`unimplemented_api_check.rs` coverage sweeps. Their presence in
`NATIVE_MODULES` with no backing crate made the bare `import * as tursodb
from "tursodb"` specifier short-circuit past file resolution and silently
claim nativeness with nothing behind it — a dangling reference, not a
working feature. The real `@perryts/tursodb` / `@perryts/iroh` packages are
consumed via their scoped specifier, which was never in `NATIVE_MODULES`
and resolves through the unrelated `node_modules`/`perry.nativeLibrary`
path, untouched here.

Regenerated `docs/src/api/reference.md`, `docs/api/perry.d.ts` (drop the
`tursodb`/`iroh` sections, keep `## fetch`), and
`docs/src/native-libraries/governance.md`'s generated table (`perry-ext-fetch`'s
package mapping drops `fetch`, keeps `node-fetch`).

Validated: cargo tests on `perry-api-manifest`, `perry-hir`, `perry-codegen`
(`manifest_consistency`), and `perry`'s `stdlib_features`/`optimized_libs`
unit tests all green; `run_lint_gates.sh` 76/77 (the one red,
"Public benchmark evidence freshness", is pre-existing on every PR);
targeted gap suite (every fixture using `node-fetch` or built-in `fetch`)
at 100% parity; hand probes confirm `import * as tursodb from "tursodb"`
now fails at compile time with a clear error instead of compiling to a
broken no-op, and built-in `fetch`/`Response`/`Headers` still resolve.
