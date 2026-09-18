// #10478: `Object.create(proto).constructor` must be the inherited
// `proto.constructor`. Perry stamps an `Object.create` result with a synthetic
// class id that only indexes its prototype object, and the `constructor`
// synthesis minted an INT32 class ref for that synthetic id instead of reading
// the chain: the value was unequal to `Object` / `A`, printed as
// `[object Function]`, had no name, and `C instanceof C` segfaulted. lodash's
// `_.isEqual(_.cloneDeep(x), x)` (baseCreate + equalObjects) hit exactly that.

const rt = <T>(v: T): T => JSON.parse(JSON.stringify(v));

class A {
  x = 1;
  m() {
    return "A.m";
  }
}
class B extends A {
  y = 2;
}
function F(this: any) {
  this.f = 1;
}
F.prototype.hello = function () {
  return "F.hello";
};
function G(this: any) {}
G.prototype = { g: 1 };

const describe = (v: any): string => {
  if (typeof v === "function") return `function ${v.name}`;
  return String(v);
};

// --- issue repro ------------------------------------------------------------------
const o: any = Object.create(Object.prototype);
const a: any = Object.create(A.prototype);
console.log("Object.create(Object.prototype).constructor === Object:", o.constructor === Object);
console.log("Object.create(A.prototype).constructor === A:", a.constructor === A);
console.log("Object.create({}).constructor === Object:", Object.create({}).constructor === Object);
console.log("({}).constructor === Object:", ({} as any).constructor === Object);
console.log("Object.getPrototypeOf(o).constructor === Object:", Object.getPrototypeOf(o).constructor === Object);
console.log("o.constructor.name:", o.constructor.name);
console.log("o.constructor instanceof Object:", o.constructor instanceof Object);
const C = o.constructor;
console.log("C instanceof C:", C instanceof C);

// --- prototype kinds ----------------------------------------------------------------
const key = rt("constructor");
const protos: [string, () => any, any][] = [
  ["Object.prototype", () => Object.prototype, Object],
  ["{}", () => ({}), Object],
  ["{a:1}", () => ({ a: 1 }), Object],
  ["JSON object", () => rt({ a: 1 }), Object],
  ["A.prototype", () => A.prototype, A],
  ["B.prototype", () => B.prototype, B],
  ["F.prototype", () => F.prototype, F],
  ["G.prototype (replaced)", () => G.prototype, Object],
  ["Array.prototype", () => Array.prototype, Array],
  ["Map.prototype", () => Map.prototype, Map],
  ["Date.prototype", () => Date.prototype, Date],
  ["new A()", () => new A(), A],
  ["new B()", () => new B(), B],
  ["Object.create(A.prototype)", () => Object.create(A.prototype), A],
  ["{constructor: F}", () => ({ constructor: F }), F],
  ["Object.create(null)", () => Object.create(null), undefined],
  ["Object.create(Object.create(null))", () => Object.create(Object.create(null)), undefined],
];
for (const [name, make, expected] of protos) {
  const obj = Object.create(make());
  const viaDot = obj.constructor;
  console.log(
    `Object.create(${name}):`,
    describe(viaDot),
    viaDot === expected,
    obj["constructor"] === expected,
    obj[key] === expected,
    "constructor" in obj,
    Object.getOwnPropertyNames(obj).length,
  );
}

// Methods still resolve through the same chain.
console.log("create(A.prototype).m():", Object.create(A.prototype).m());
console.log("create(B.prototype).m():", Object.create(B.prototype).m());
console.log("create(F.prototype).hello():", Object.create(F.prototype).hello());
console.log("new (create(A.prototype).constructor)() instanceof A:", new (Object.create(A.prototype).constructor)() instanceof A);
console.log("create(A.prototype) instanceof A:", Object.create(A.prototype) instanceof A);
console.log("create(B.prototype) instanceof A:", Object.create(B.prototype) instanceof A);

// Constructed instances are unaffected.
console.log("new A().constructor === A:", new A().constructor === A);
console.log("new B().constructor === B:", new B().constructor === B);
console.log("new F().constructor === F:", new (F as any)().constructor === F);
console.log("new G().constructor === Object:", new (G as any)().constructor === Object);

// An own `constructor` on the created object still wins.
const own: any = Object.create(A.prototype);
own.constructor = F;
console.log("own constructor wins:", own.constructor === F);

// --- the prototype link itself stays authoritative ------------------------------------
// `Object.getPrototypeOf` must keep answering the exact object passed to
// `Object.create`, and an inherited accessor / non-writable slot must keep
// resolving through it (both are reached from the same class-id link that now
// wins over the `constructor`-derived guess).
const accessorProto: any = {};
let setterSum = 0;
Object.defineProperty(accessorProto, "acc", {
  get() {
    return 41;
  },
  set(v: number) {
    setterSum += v;
  },
});
Object.defineProperty(accessorProto, "frozenField", { value: "proto", writable: false });
const viaCreate: any = Object.create(accessorProto);
console.log("getPrototypeOf identity:", Object.getPrototypeOf(viaCreate) === accessorProto);
console.log("inherited getter:", viaCreate.acc);
viaCreate.acc = 1;
viaCreate.acc = 2;
console.log("inherited setter:", setterSum, "own acc:", Object.getOwnPropertyNames(viaCreate).length);
try {
  viaCreate.frozenField = "written";
  console.log("inherited non-writable: silent", viaCreate.frozenField);
} catch (e: any) {
  console.log(`inherited non-writable: ${e.constructor.name}`, viaCreate.frozenField);
}
const fnInstance: any = new (F as any)();
console.log("getPrototypeOf(new F()) === F.prototype:", Object.getPrototypeOf(fnInstance) === F.prototype);
console.log("getPrototypeOf(new G()) === G.prototype:", Object.getPrototypeOf(new (G as any)()) === G.prototype);
console.log("getPrototypeOf(new A()) === A.prototype:", Object.getPrototypeOf(new A()) === A.prototype);
function H(this: any) {}
(H as any).prototype = { late: 1 };
// (An instance created BEFORE the reassignment keeps the old prototype in Node;
// Perry's per-class-id prototype link re-points it. Pre-existing divergence,
// unrelated to this fix, so only the post-reassignment instance is asserted.)
console.log("reassigned prototype:", Object.getPrototypeOf(new (H as any)()) === (H as any).prototype, new (H as any)().late);

// --- lodash-style checks ---------------------------------------------------------------
// baseCreate: `Object.create(Object.getPrototypeOf(value))`.
const baseCreate = (value: any) => Object.create(Object.getPrototypeOf(value));
// equalObjects' constructor gate (lodash.js 4.18.1).
function constructorsDiffer(object: any, other: any): boolean {
  const objCtor = object.constructor;
  const othCtor = other.constructor;
  return (
    objCtor != othCtor &&
    "constructor" in object &&
    "constructor" in other &&
    !(
      typeof objCtor == "function" &&
      objCtor instanceof objCtor &&
      typeof othCtor == "function" &&
      othCtor instanceof othCtor
    )
  );
}
const samples: [string, any][] = [
  ["plain", { a: 1, b: [1, 2] }],
  ["json", rt({ a: 1 })],
  ["class A", new A()],
  ["class B", new B()],
  ["function F", new (F as any)()],
];
for (const [name, value] of samples) {
  const clone = baseCreate(value);
  Object.assign(clone, value);
  console.log(
    `baseCreate(${name}):`,
    clone.constructor === value.constructor,
    constructorsDiffer(clone, value),
    Object.getPrototypeOf(clone) === Object.getPrototypeOf(value),
  );
}
const ctor = baseCreate({}).constructor;
console.log("typeof ctor:", typeof ctor, "ctor instanceof ctor:", ctor instanceof ctor);
const isPlainObjectLike = (v: any) => {
  const proto = Object.getPrototypeOf(v);
  if (proto === null) return true;
  const Ctor = Object.prototype.hasOwnProperty.call(proto, "constructor") && proto.constructor;
  return typeof Ctor == "function" && Ctor instanceof Ctor && Ctor === Object;
};
console.log(
  "isPlainObjectLike:",
  isPlainObjectLike({}),
  isPlainObjectLike(baseCreate({})),
  isPlainObjectLike(Object.create(null)),
  isPlainObjectLike(new A()),
  isPlainObjectLike(Object.create(A.prototype)),
);
