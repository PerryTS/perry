// Issue #886: `const X = Object.defineProperty; X(target, name, descriptor)`
// threw `TypeError: value is not a function` at runtime because the
// `Object.<staticMethod>` recogniser in `lower_call.rs` only fired on the
// literal member-expression callee shape, not on an indirect call through a
// local alias. esbuild's CJS-bundle prelude aliases the static methods to
// short locals (`__defProp`, `__defProps`, `__getOwnPropDesc`, `__getProtoOf`,
// `__getOwnPropNames`) so every esbuild-bundled npm package failed at module
// init.
//
// The fix (PR following this test): detect the alias at HIR-lowering time in
// `destructuring.rs::lower_var_decl_with_destructuring` and synthesize the
// matching dedicated HIR variant at the indirect call site in
// `lower/expr_call.rs`. Whitelisted method names mirror the
// `obj_name == "Object"` recogniser arm.
//
// FOLLOWUP (out of scope for #886): destructuring aliases —
// `const { defineProperty } = Object;` — go through a different code path
// (object-pattern destructuring) and are NOT covered by this fix. They will
// still throw at runtime; covered separately when esbuild's emit shows up.

// 1. Direct esbuild emit pattern (verbatim from the #886 issue body).
//    The PRE-FIX failure was `TypeError: value is not a function` thrown
//    by `__defProp(...)` itself — control never reached the trailing
//    `console.log("ok")`. POST-FIX the indirect call dispatches through
//    `Expr::ObjectDefineProperty` and the program exits cleanly.
//
//    The defined accessors themselves read back as `undefined` here
//    because of an UNRELATED for-in / getter-capture interaction — see
//    test 1b below for a value-descriptor variant that does observe the
//    defined property. The `console.log("ok")` line is the actual
//    regression guard for #886; everything else is bonus coverage.
const __defProp = Object.defineProperty;
const __export = (target: Record<string, any>, all: Record<string, any>) => {
    for (let name in all)
        __defProp(target, name, { get: all[name], enumerable: true });
};
__export({}, { a: () => 1 });

// 1b. Same alias, value descriptor — fully observable round trip.
const targetA: Record<string, any> = {};
__defProp(targetA, "value_a", { value: 100, enumerable: true });
__defProp(targetA, "value_b", { value: 200, enumerable: true });
console.log("value_a=" + targetA.value_a);
console.log("value_b=" + targetA.value_b);

// 2. Alias propagated through `const Y = X`.
const __defProp2 = __defProp;
const targetB: Record<string, any> = {};
__defProp2(targetB, "c", { value: 42, enumerable: true });
console.log("c=" + targetB.c);

// 3. Other aliased static methods. Each must dispatch through its dedicated
//    HIR variant; pre-fix all of these threw on the indirect call.
const __setProto = Object.setPrototypeOf;
const protoTarget: Record<string, any> = { x: 1 };
__setProto(protoTarget, { y: 2 });
console.log("x=" + protoTarget.x);

const __keys = Object.keys;
const __values = Object.values;
const __entries = Object.entries;
const src = { p: 10, q: 20 };
console.log("keys=" + __keys(src).join(","));
console.log("values=" + __values(src).join(","));
console.log("entries=" + JSON.stringify(__entries(src)));

const __assign = Object.assign;
const dst = { existing: 1 };
__assign(dst, { added: 2 });
console.log("assign=" + JSON.stringify(dst));

const __create = Object.create;
const fresh = __create(null);
fresh.field = "hello";
console.log("create=" + fresh.field);

// 4. Final guard line — if any of the above threw, we won't reach this.
console.log("ok");
