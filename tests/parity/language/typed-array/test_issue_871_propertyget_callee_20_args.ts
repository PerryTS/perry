// Issue #871 (part 2): codegen used to bail with
// `Call callee shape not supported (PropertyGet) with 20 args` when uuid's
// `sha1.js` calls `Uint8Array.of(b0, b1, ..., b19)` with 20 arguments.
//
// Part 1 (HIR named-default-function lowering) was closed by PR #890. This
// regression test pins the part-2 fix: `Uint8Array.of(...)` lowers to
// `Expr::Uint8ArrayFrom(Expr::Array(args))` in the HIR, so the 20-arg call
// site avoids the closure-call dispatch tower's 16-arg ceiling entirely
// (there's no `js_closure_call17..20`) and produces a real Uint8Array of
// the given bytes.
const u = Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20);
console.log("len:", u.length, "first:", u[0], "last:", u[19]);
