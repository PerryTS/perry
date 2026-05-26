// #26 / #321: symbol-keyed prototype-chain lookup across MULTIPLE levels of
// `Object.create`. This is effect's `Either`/`ParseResult` brand pattern:
//
//   const TypeId = Symbol.for("effect/Either")
//   const CommonProto = { [TypeId]: {...} }
//   const RightProto  = Object.assign(Object.create(CommonProto), { _tag: "Right" })
//   const right = (r) => { const a = Object.create(RightProto); a.right = r; return a }
//
// `isEither(x) = TypeId in x` must walk a→RightProto→CommonProto and find the
// brand TWO links up. Pre-fix, `resolve_proto_chain_symbol` advanced the chain
// only via `parent_class_id` (the `class A extends B` axis); a synthetic
// `Object.create(proto)` class id has no parent, so the walk stopped after the
// first prototype object. `TypeId in a` / `a[TypeId]` therefore missed the
// brand, making `ParseResult.isEither(te)` false for every struct-property
// parse — breaking `S.is`/`decodeUnknownSync`/`encodeSync` on a `Struct`.
//
// Fix: the symbol-chain walk now follows the prototype object's OWN class id
// (the `Object.create` link) in addition to `parent_class_id`.
//
// Note: string-keyed reads (`_tag`) already worked because the string-chain
// walk recurses through the full field getter at each prototype object.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

const TypeId: unique symbol = Symbol.for("perry-test-Either") as any;

const CommonProto: any = { [TypeId]: { _R: 1 } };
const RightProto: any = Object.assign(Object.create(CommonProto), { _tag: "Right" });
const LeftProto: any = Object.assign(Object.create(CommonProto), { _tag: "Left" });

const right = (r: unknown) => {
  const a: any = Object.create(RightProto);
  a.right = r;
  return a;
};
const left = (l: unknown) => {
  const a: any = Object.create(LeftProto);
  a.left = l;
  return a;
};

const isEither = (u: any) => u != null && (TypeId in u);
const isRight = (u: any) => u._tag === "Right";

const r = right(42);
const l = left("err");

// (1) two-level symbol-in: a -> RightProto -> CommonProto (brand on CommonProto).
console.log("TypeId in r:", TypeId in r);          // expect true
console.log("TypeId in l:", TypeId in l);          // expect true

// (2) symbol READ across two levels.
console.log("r[TypeId] obj:", typeof (r as any)[TypeId]); // expect object

// (3) string key still works (one level: own on RightProto).
console.log("r._tag:", r._tag, "l._tag:", l._tag); // expect Right Left

// (4) negative: an unrelated plain object is NOT branded.
console.log("TypeId in {}:", TypeId in ({} as any)); // expect false

// (5) the actual isEither/isRight guard shape effect uses.
console.log("isEither(r):", isEither(r), "isRight(r):", isRight(r)); // expect true true
console.log("isEither({a:1}):", isEither({ a: 1 } as any));          // expect false

// (6) three-level chain (a -> mid -> proto, brand on the top proto).
const Sym2: unique symbol = Symbol.for("perry-test-Sym2") as any;
const top: any = { [Sym2]: 7 };
const mid: any = Object.create(top);
const deep: any = Object.create(mid);
console.log("Sym2 in deep:", Sym2 in deep);    // expect true
console.log("deep[Sym2]:", (deep as any)[Sym2]); // expect 7
