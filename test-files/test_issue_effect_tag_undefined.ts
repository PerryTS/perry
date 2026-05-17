// Issue #894: effect's Schema.ts crashed during module init with
// `TypeError: Cannot read properties of undefined (reading '_tag')`.
//
// Root cause: a class returned from a factory function with a static
// computed-Symbol-key field (`static [TypeId] = variance`) lost that
// field after the factory call. The pre-fix codegen emitted the
// `js_class_register_static_symbol` registration once at module-init
// time, BEFORE the module's top-level lets had been assigned. Both
// the key (`Symbol.for("…")`) and the value (`variance = {…}`) were
// read from their then-uninitialised module globals — registration
// recorded `(class_id, 0, 0)`, which `class_static_symbol_lookup`
// silently couldn't match against the real key at call-time.
//
// Effect's `function make(ast) { return class { static [TypeId] =
// variance } }` factory is the canonical case — the class is returned
// from `make()` and used as a class-extends parent throughout Schema.ts.
// `isSchema(C) = hasProperty(C, TypeId) && isObject(C[TypeId])` then
// returned false on the freshly-returned class. Effect's `dual`
// dispatch fell to the curried path, downstream `class extends
// transform(...)` etc. unwound through unexpected shapes, and a
// `make()` call eventually received `undefined` as its `ast` argument,
// producing the `_tag` TypeError.
//
// The fix has two parts:
//
//  1. Move the per-class static-field initialisation from
//     `init_static_fields` (pre-user-init) to a new
//     `init_static_fields_late` (post-user-init). This handles
//     TOP-LEVEL classes — their static-field globals see populated
//     module lets when the late phase emits the store.
//
//  2. For class EXPRESSIONS returned from factory functions, sequence
//     a new `Expr::RegisterClassStaticSymbol` in front of the
//     `Expr::ClassRef` so each factory invocation re-emits the
//     `js_class_register_static_symbol(class_id, key, value)` with
//     CURRENT values of the captured/module-level free variables.
//
// The standalone shape below mirrors effect's `make()` factory.
//
// Note: the full effect smoke (`import { Effect } from "effect";
// console.log(typeof Effect.succeed)`) is still blocked downstream
// on (a) `arguments.length` reading 0 inside FnExpr closures returned
// from another function (perry's synthetic `...arguments` rest param
// only captures TRAILING args, not all args — so `function(a, b) {
// arguments.length }` called with two args sees `arguments.length ===
// 0`), and (b) static methods on classes returned from a factory not
// being routable via dynamic property access (`(make()).pipe` reads
// `undefined`). Both are separate gaps to file as follow-ups.

const TypeId: unique symbol = Symbol.for("perry/test/issue_894") as any;

const variance = {
  _A: (x: any) => x,
  _I: (x: any) => x,
};

function makeSchemaClass(ast: any) {
  return class SchemaClass {
    static ast = ast;
    static [TypeId] = variance;
  };
}

const isObject = (x: any) =>
  (typeof x === "object" && x !== null) || typeof x === "function";

const hasProperty = (u: any, prop: any): boolean =>
  isObject(u) && (prop in u);

const isSchemaLike = (u: any) =>
  hasProperty(u, TypeId) && isObject((u as any)[TypeId]);

const FakeAst1 = { _tag: "Alpha" };
const A = makeSchemaClass(FakeAst1);

console.log("typeof A:", typeof A);
console.log("TypeId in A:", TypeId in A);
console.log("A[TypeId] is object:", isObject((A as any)[TypeId]));
console.log("isSchemaLike(A):", isSchemaLike(A));

// Each factory invocation must re-register so a SECOND call with a
// different ast still passes the same isSchema check.
const FakeAst2 = { _tag: "Beta" };
const B = makeSchemaClass(FakeAst2);
console.log("isSchemaLike(B):", isSchemaLike(B));
