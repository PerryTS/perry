// epic #1785 / #1758: static SYMBOL property lookup on class-object values —
// both the `in` operator (`Predicate.hasProperty`) and the read (`obj[Sym]`),
// for OWN props and props INHERITED from a class-expression parent.
//
// Two bugs:
//   (b) `Sym in classObject` returned false even for an OWN static `[Sym]`
//       (the `in` operator's generic path rejected non-string keys outright).
//   (c) `Sub[Sym]` where `class Sub extends make(...)` returned undefined
//       (inherited static symbols weren't walked through the prototype chain).
//
// This breaks effect's `isSchema(u) = hasProperty(u, TypeId) && isObject(u[TypeId])`:
// `BigIntFromSelf = class extends make(bigIntKeyword) {}` carries `[TypeId]`
// only via inheritance, so `transformOrFail`'s `dual` predicate
// (`isSchema(args[0]) && isSchema(args[1])`) returned false and `transformOrFail`
// degraded to a curried function — never building the transformation class.
//
// Fix: `js_object_has_property` delegates symbol keys to the symbol resolver;
// `js_object_get_symbol_property` walks the `CLASS_PROTOTYPE_OBJECTS` chain
// (new `resolve_proto_chain_symbol`) for both class-ref (INT32) and
// class-object (POINTER) receivers.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

const TypeId: unique symbol = Symbol.for("perry-test-TypeId") as any;
const variance = { _A: 1 };

function make(a: any) {
  return class S {
    [TypeId] = variance;
    static ast = a;
    static [TypeId] = variance;
  };
}

// (1) OWN static symbol on a class-object value (POINTER class-object).
const M = make({ _tag: "y" });
console.log("(1) read  M[TypeId]:", typeof (M as any)[TypeId]);
console.log("(1) in    TypeId in M:", (TypeId as any) in (M as any));

// (2) INHERITED static symbol on a subclass (INT32 class ref extends make()).
class Sub extends make({ _tag: "x" }) {}
console.log("(2) read  Sub[TypeId]:", typeof (Sub as any)[TypeId]);
console.log("(2) in    TypeId in Sub:", (TypeId as any) in (Sub as any));

// (3) a symbol that is NOT present must report absent (no false positives).
const Other: unique symbol = Symbol.for("perry-test-Other") as any;
console.log("(3) read  M[Other]:", typeof (M as any)[Other]);
console.log("(3) in    Other in M:", (Other as any) in (M as any));

// (4) effect's `isSchema` shape: hasProperty(u, TypeId) && isObject(u[TypeId]).
const isSchemaLike = (u: any) =>
  u != null && (TypeId as any) in u && typeof u[TypeId] === "object";
console.log("(4) isSchemaLike(M):", isSchemaLike(M));
console.log("(4) isSchemaLike(Sub):", isSchemaLike(Sub));
console.log("(4) isSchemaLike({}):", isSchemaLike({}));
