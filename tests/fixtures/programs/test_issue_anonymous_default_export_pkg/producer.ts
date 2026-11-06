// Anonymous-default-function regression: `export default function () { ... }`
// (no name binding). Pre-fix the HIR lowerer dropped the body entirely,
// codegen never emitted `perry_fn_<src>__default`, and any consumer
// link-failed with `Undefined symbols: _perry_fn_<src>__default` — same
// shape that blocks zod's `v4/locales/en.ts` and vitest's bundled CJS
// under `perry.compilePackages`.
export default function () {
  return 42;
}
