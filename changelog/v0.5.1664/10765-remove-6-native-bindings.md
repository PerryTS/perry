Removed six native npm-package bindings so real npm source compiles instead
of a hand-written Rust reimplementation: `qs` (6.15.3), `fastify` (5.10.0),
`dayjs` (1.11.21), `date-fns` (4.4.0), `rate-limiter-flexible` (11.2.0), and
`node-cron` (4.6.0). Each package's primary documented use compiles with no
`perry.compilePackages` entry and matches `node --experimental-strip-types`
byte-for-byte on its pinned version.

`dayjs`/`date-fns` shared one crate (`perry-ext-dayjs`, deleted) and had a
hidden stdlib-side duplicate (`perry-stdlib/src/dayjs.rs`, #10678-shaped —
also deleted, along with the `bundled-dayjs` feature). `rate-limiter-flexible`
had the same duplicate pattern (`perry-ext-ratelimit` +
`perry-stdlib/src/ratelimit.rs` + the `bundled-ratelimit`/`rate-limit`
features + the now-unused `governor` dependency); its orphaned
`js_ratelimit_create` stub (declared and stubbed everywhere, implemented
nowhere) went with it. `node-cron` shares `perry-ext-cron` with the `cron`
package, which is unaffected and stays — only node-cron's own
well-known-binding entry, manifest rows, and `("node-cron", "schedule") =>
"CronJob"` HIR tagging were removed; the deleted `test_parity_cron.ts` had a
known-failure entry documenting that `cron.validate`/`cron.schedule` resolved
as `undefined` when imported from `"node-cron"` — a real, tracked defect in
the native path this removes. `fastify` had no in-stdlib fallback but did
have dynamic-dispatch adapters (`external-fastify-pump`) that are now dead
weight and removed with it.

`typescript` was in an earlier revision of this change and has been **pulled
back out**. Its binding, crate, manifest rows and HIR enum folding are all
retained unchanged. Removing it makes perry execute real `typescript.js` for
the first time — `well_known.rs` routes `import 'X'` to the bundled wrapper
even when `node_modules/X` is on disk, so that path is unreachable today —
and real `typescript.js` hits a compiler defect (`TypeError: Cannot convert
undefined or null to object` inside `InferredProject.addRoot`). The defect
predates this change and is not caused by it, but a path that works for users
today would start throwing, so the removal waits for the compiler fix.

Recomputed from the resolved tree (rebased onto v0.5.1608, after #10693's
nanoid removal): workspace members 76→72, `externalize` 27→23, `keep`
unchanged at 44 (four crates deleted; `perry-ext-typescript` keeps its
`externalize`/`compile-source` classification). API manifest 3006→2923 entries across 132→126 modules
(`docs/api/perry.d.ts` 2064→2043 across 130→124), regenerated from the
built binary's `--print-api-manifest`, not hand-edited. `native_result_ledger.py` 376→369
rows / 326→320 providers. `unrooted_local_shape_baseline.json` 578→562 and
`string_payload_access_baseline.txt` regenerated (decreases only).
`binding_governance.py` 30 extension crates classified; `binding_pins.mjs`
26 pinned, lock-step holds (unchanged — `typescript`'s upstream pin is
retained along with its binding).
