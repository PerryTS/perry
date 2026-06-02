// Regression for the anonymous-default-function lowering gap that
// blocks zod (`v4/locales/en.ts`) and vitest under
// `perry.compilePackages`. Pre-fix `export default function () { ... }`
// (no name binding) was dropped entirely by the HIR lowerer — codegen
// never emitted `perry_fn_<src>__default`, so the consumer link-failed
// with `Undefined symbols: _perry_fn_<src>__default`. The
// `__perry_wrap_perry_fn_<src>__default` rename wrapper (added in #837)
// also had nothing to point at.
//
// Output must match `node --experimental-strip-types` byte-for-byte.

import x from "./test_issue_anonymous_default_export_pkg/producer.ts";

console.log(x());
