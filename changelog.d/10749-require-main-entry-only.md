Fixed `require.main === module` being trivially `true` in **every** compiled
CommonJS module, not just the process entry point (#10735).
`cjs_wrap`'s preamble unconditionally emitted `require.main = module;`, so
the standard "am I the entry, or merely imported?" idiom took its CLI
branch in every dependency that used it — including bundled packages
(dotenv 18.0.1's `dist/index.cjs` is a confirmed real-world example).

The fix threads the compiler's existing entry-module knowledge (the same
comparison `import.meta.main` uses) through `cjs_wrap`, but that alone is
insufficient: `cjs_wrap` transpiles a statically-known
`require('./relative')` into a hoisted ESM import, and ESM import
evaluation runs a module's static-import dependencies *before* the
importing module's own top-level code. So a CJS entry's own dependencies
initialize before the entry's own preamble runs, which means "the entry
publishes `require.main` in its own preamble" is too late for any
dependency reached via a hoisted static import. Fixed by publishing a
placeholder object as the shared "main module" from the program's `main()`
itself, before any module's `__init` runs at all (gated on the entry being
CJS-wrapped, so an ESM entry correctly leaves `require.main` `undefined`
for CJS modules it imports); the entry later reclaims that exact object
and fills in its real fields, preserving identity for dependencies that
captured `require.main` before the entry's own code ran.

The new runtime-side cache backing this (`CJS_MAIN_MODULE`) is registered
with the GC's mutable-root-scanner machinery and verified under forced
evacuation (`PERRY_GC_SCHEDULE_SEED`/`PERRY_GC_FORCE_EVACUATE`/
`PERRY_GC_PROTECT_FROMSPACE`): the placeholder moved, the cache followed
it, and every identity assertion held across 8,006 forced collections. A
new test (`gc::tests::cjs_main_module`) asserts the rewrite counter is
non-zero under a real evacuating minor, so a future regression that stops
rewriting the cache fails a test instead of silently reintroducing this
bug's failure mode.
