Fixed `domain.active`/`domain._stack` freezing at their first-read value on
the default `node:domain` import (`import domain from "node:domain"`):
`d.enter(); domain.active; d.exit(); domain.active` kept returning the
entered domain instead of `undefined`, and `domain._stack.length` stayed at
1. `enter()`/`exit()` themselves were correct — only the READ was stale.

Root cause: aaa3ba534 ("fix(module): complete Node 26 parity for node:module
(#7312)") added an ESM default/named export snapshot cache
(`NATIVE_ESM_EXPORT_VALUES` in `native_module_export_value`,
`crates/perry-runtime/src/object/native_module.rs`) that memoizes the first
non-`undefined` value read for a native-module property, refreshed only by
`module.syncBuiltinESMExports()`. `domain.active`/`domain._stack` are not
constants — the `"domain"` arm of `get_native_module_constant`
(`crates/perry-runtime/src/object/native_module/constants.rs`) resolves them
through a dispatch call (`JS_NATIVE_DOMAIN_DISPATCH`) on every read — so the
snapshot cache froze them at whatever value the dispatch call happened to
return on first access.

Fix: `native_module_constant_is_live(module, property)` marks
`("domain", "_stack")`/`("domain", "active")` as live, dispatch-backed reads;
`native_module_export_value` now skips both the cache lookup and the cache
write for a live property, so it is re-evaluated on every read while genuine
constants keep the snapshot-cache fast path. `require("domain")` and
`process.domain` already read through a separate, uncached dynamic-field
path and were unaffected; `import * as domain from "node:domain"` shares
this same live-read fix but Node itself freezes `.active` on that import
form at its initial value until `syncBuiltinESMExports()` (a pre-existing,
separate ESM/CJS interop gap, out of scope here — not covered by any
node-suite test).

Validation: node-suite `domain` 13/48 → 18/48 (fast mode,
`PERRY_NO_AUTO_OPTIMIZE=1`), recovering exactly the 5 regressed tests
(`bind-call-contract`, `enter-duplicate`, `enter-exit-active`,
`intercept-call-contract`, `run-nested-restore`); `module`/`process`/`events`/
`globals`/`util` unchanged. New regression test:
`test-files/test_gap_domain_active_stack_live_reads.ts` (fails before the
fix, byte-for-byte matches `node --experimental-strip-types` after).
`cargo test -p perry-runtime native_module`: 37/37 passing.
