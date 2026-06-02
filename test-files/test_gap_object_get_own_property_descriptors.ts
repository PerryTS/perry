// Object.getOwnPropertyDescriptors(obj) — the PLURAL form. The singular
// `Object.getOwnPropertyDescriptor(obj, key)` already had a dedicated HIR
// variant + runtime helper, but the plural had none: the literal-callee
// recogniser in `lower/expr_call/native_module.rs` (and the esbuild-alias
// recogniser in `destructuring/var_decl.rs`) silently fell through, so codegen
// lowered the callee to a constant null and the call threw
// `TypeError: value is not a function`.
//
// This is the first blocker on `import * as S from "effect/Schema"` (#1791 /
// #1758 / #1785): effect's `SchemaAST.annotations` clones an AST node via
//   Object.create(Object.getPrototypeOf(ast), Object.getOwnPropertyDescriptors(ast))
// during module init, so Schema.ts threw before any user code ran.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// 1. Plain data object — every key reports a full data descriptor.
const o = { a: 1, b: "x", c: true };
console.log(JSON.stringify(Object.getOwnPropertyDescriptors(o)));

// 2. Empty object — empty descriptor map.
console.log(JSON.stringify(Object.getOwnPropertyDescriptors({})));

// 3. Alias / indirect-call form (esbuild CJS prelude shape).
const gopds = Object.getOwnPropertyDescriptors;
console.log(JSON.stringify(gopds({ z: 9 })));

// 4. Accessor (getter) property — descriptor carries `get`, not `value`.
const acc: any = {};
Object.defineProperty(acc, "g", {
  get() {
    return 42;
  },
  enumerable: true,
  configurable: true,
});
const d = Object.getOwnPropertyDescriptors(acc);
console.log("has get:", typeof d.g.get, "enumerable:", d.g.enumerable);

// 5. The exact effect shape: shallow-clone an object (own props as
//    descriptors) while preserving the prototype chain.
const proto = { kind: "base" };
const src: any = Object.create(proto);
src.val = 7;
const clone: any = Object.create(
  Object.getPrototypeOf(src),
  Object.getOwnPropertyDescriptors(src),
);
console.log("clone.val:", clone.val, "proto.kind:", clone.kind);

// 6. Result is a real object usable downstream.
console.log("typeof result:", typeof Object.getOwnPropertyDescriptors(o));
