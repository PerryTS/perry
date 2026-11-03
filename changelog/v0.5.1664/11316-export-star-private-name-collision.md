Fixed a link failure for re-exports that travel through an `export *` barrel
when another star source has a private binding with the same name
(`Undefined symbols: _perry_fn_<barrel>__<name>`). `export *` forwards only a
source module's exports, but the binding-origin walk in
`crates/perry-hir/src/dynamic_import/binding_origin.rs` treated any same-named
function, class, global, enum, `let`, or import as the module's definition of
the exported name. A private `foo` in one star source and an exported `foo` in
another then looked ambiguous, the walk gave up, and the namespace entry named
the pure barrel, which never emits that getter. On export-name hops (import,
re-export, and star) a same-named binding now resolves only when the module
exports it under that name, which also stops a re-export from silently binding
to a private function of a star forwarder.

Real-world trigger: zod 4.6.5, where `core/schemas.ts` has a private
`validateAsync` next to the exported one in `core/parse.ts`, and
`classic/parse.ts` re-exports it from `core/index.js`.

Coverage: `test-parity/node-suite/module/imports/export-star-private-name-collision.ts`
(with fixtures under `imports/fixtures/export-star-private/`), two
`perry-hir` unit tests, and a tier-3 package fixture pinning zod 4.6.5
(`tests/release/packages/zod-4-6/`).
