// Regression for the uuid `__perry_wrap_perry_fn_<src>__default` link
// error that PR #903 unmasked.
//
// PR #903 corrected default-import resolution so two `import X from "./a";
// import Y from "./b"` in the same file no longer collide on the
// shared `import_function_prefixes["default"]` key. Pre-fix the
// collision masked a separate codegen bug in uuid's `sha1.js`
// (`Uint8Array.of` with 20 args bails out, so the module never
// emits its wrapper symbol). Post-fix the link error surfaces with:
//
//   Undefined symbols: ___perry_wrap_perry_fn_<sha1>__default,
//     referenced from ___perry_wrap_perry_fn_<v5>__v5
//
// This test verifies the named-default-decl shape `export default
// function foo() {}` (PR #890) still resolves its wrapper symbol
// correctly post-#903 — i.e. the consumer-side `import foo from
// "./producer"` lowers to a reference that producer-side codegen
// satisfies via the `Export::Named { local: "foo", exported: "default" }`
// alias path.
//
// Pairs with `test_issue_uuid_cross_module_fn.ts` (the original #890
// regression). Output must match `node --experimental-strip-types`
// byte-for-byte.
import sha1 from "./fixtures/issue_uuid_sha1_default_export_regression/producer.ts";

console.log(sha1());
