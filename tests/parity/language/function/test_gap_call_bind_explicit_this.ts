// Forwarding with an explicit `this`: Function.prototype.call / apply / bind
// and bound functions. Receivers are coerced per the callee (sloppy boxes a
// primitive, strict does not), `this`-capturing object-literal methods are
// rebound, and heap arguments survive allocation on the way (the callees and
// getters below churn the nursery to make a collection likely).

const $call = Function.prototype.call;
const $bind = Function.prototype.bind;
const $apply = Function.prototype.apply;
const uncurry = (fn: Function): any => Reflect.apply($bind, $call, [fn]);

function churn(n: number): number {
  let keep: number[][] = [];
  for (let i = 0; i < n; i++) keep.push([i, i * 2, i * 3]);
  return keep.length;
}

const sloppyThis = new Function(
  "a",
  "b",
  "return [typeof this, this instanceof Object, String(this.valueOf()), a.v, b.v].join('|');",
);
function strictThis(this: unknown, a: { v: string }, b: { v: string }): string {
  "use strict";
  churn(2000);
  return [typeof this, String(this), a.v, b.v].join("|");
}

const heapA = { v: "A" + churn(10) };
const heapB = { v: "B" + churn(10) };
const primitives: unknown[] = [5, "str", true, Symbol.for("sym"), 10n];
for (const prim of primitives) {
  let sloppy: string;
  try {
    sloppy = sloppyThis.call(prim, heapA, heapB);
  } catch (e) {
    sloppy = "threw " + (e as Error).constructor.name;
  }
  let strict: string;
  try {
    strict = strictThis.call(prim, heapA, heapB);
  } catch (e) {
    strict = "threw " + (e as Error).constructor.name;
  }
  console.log("call", typeof prim, sloppy, strict);
  console.log("apply", typeof prim, strictThis.apply(prim, [heapA, heapB]) === strict);
  console.log("uncurried", typeof prim, uncurry(strictThis)(prim, heapA, heapB) === strict);
}

// Object-literal method that keeps `this` in a capture.
const holder = {
  tag: "holder",
  join(this: any, a: { v: string }, b: { v: string }, c: { v: string }): string {
    churn(3000);
    return [this.tag, a.v, b.v, c.v].join("+");
  },
};
const other = { tag: "other" };
const parts = [{ v: "p1" }, { v: "p2" }, { v: "p3" }];
console.log("method call:", holder.join.call(other, parts[0], parts[1], parts[2]));
console.log("method apply:", holder.join.apply(other, parts));
console.log("method uncurried:", uncurry(holder.join)(other, parts[0], parts[1], parts[2]));
console.log("method bound:", holder.join.bind(other, parts[0])(parts[1], parts[2]));
console.log("method spread:", holder.join.call(...([other, ...parts] as [any, any, any, any])));

// bind: sloppy target, primitive thisArg, partial heap args.
const boundSloppy = $bind.call(sloppyThis, 42, { v: "partial" });
churn(5000);
console.log("bind sloppy:", boundSloppy({ v: "late" }));
console.log("bind strict:", strictThis.bind("s", { v: "x" })({ v: "y" }));

// bind: a target whose `name` getter allocates.
function named(this: any, a: unknown, b: unknown): string {
  return String(this && this.k) + ":" + a + ":" + b;
}
Object.defineProperty(named, "name", {
  get() {
    churn(4000);
    return "dyn" + "Name";
  },
});
const boundNamed = named.bind({ k: "K" }, { toString: () => "first" });
console.log("bind name getter:", boundNamed.name, boundNamed("second"), boundNamed.length);

// new on a bound function: partial + call-time args, instanceof, order.
class Point {
  xs: unknown[];
  constructor(...xs: unknown[]) {
    this.xs = xs;
  }
}
const BoundPoint = Point.bind(null, "a", "b");
const bp = new (BoundPoint as any)("c", "d");
console.log("new bound:", bp instanceof Point, bp instanceof (BoundPoint as any), bp.xs.join(","));
const BoundPoint2 = $bind.call(Point, null, "x");
const bp2 = new BoundPoint2("y");
console.log("new $bind.call:", bp2 instanceof Point, bp2.xs.join(","));
function Legacy(this: any, a: unknown, b: unknown, c: unknown) {
  this.args = [a, b, c].join(",");
}
const BoundLegacy = (Legacy as any).bind({ ignored: true }, 1);
const bl = new BoundLegacy(2, 3);
console.log("new bound function:", bl instanceof (Legacy as any), bl.args, bl.ignored);

// bound-of-bound.
function three(this: any, a: number, b: number, c: number): string {
  return String(this && this.id) + ":" + [a, b, c].join(",");
}
const bb = three.bind({ id: "a" } as any, 1).bind({ id: "b" } as any, 2);
console.log("bound of bound:", bb(3), bb.name, bb.length);
function Ctor3(this: any, a: number, b: number, c: number) {
  this.v = [a, b, c].join(",");
}
const BB = (Ctor3 as any).bind({}, 1).bind({}, 2);
const bbo = new BB(3);
console.log("new bound of bound:", bbo.v, bbo instanceof (Ctor3 as any));

// Class constructors have no [[Call]].
class C {
  constructor() {}
}
const tries: [string, () => unknown][] = [
  ["C.call", () => (C as any).call({})],
  ["C.apply", () => (C as any).apply({}, [])],
  ["$call.call(C)", () => $call.call(C, {})],
  ["uncurry(C)", () => uncurry(C)({})],
  ["Reflect.apply($call)", () => Reflect.apply($call, C, [{}])],
  ["bound C()", () => (C.bind(null) as any)()],
];
for (const [label, f] of tries) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}
const CExpr = class {};
try {
  $call.call(CExpr, {});
  console.log("class expr no throw");
} catch (e) {
  console.log("class expr", (e as Error).constructor.name);
}

// call and bind are not constructors.
for (const [label, f] of [
  ["new call", () => new ($call as any)()],
  ["new bind", () => new ($bind as any)()],
  ["new apply", () => new ($apply as any)()],
] as [string, () => unknown][]) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

// `arguments` in a callee reached through call.
function sloppyArgs(this: any, a: any) {
  const before = arguments.length;
  arguments[0] = "changed";
  return [before, a, arguments[0], Array.isArray(arguments), typeof arguments].join("|");
}
function strictArgs(this: any, a: any) {
  "use strict";
  arguments[0] = "changed";
  return [arguments.length, a, arguments[0]].join("|");
}
const argObj = { v: 1 };
console.log("sloppy arguments:", sloppyArgs.call(null, "orig", argObj));
console.log("strict arguments:", strictArgs.call(null, "orig", argObj));
function argsIdentity(this: any) {
  return arguments;
}
const argsOut = argsIdentity.call(null, argObj, 2);
console.log("arguments identity:", argsOut[0] === argObj, argsOut.length);

// Spread into call.
function who(this: any, a: number, b: number): string {
  return String(this.name) + ":" + (a + b);
}
const spreadArgs: [any, number, number] = [{ name: "spread" }, 1, 2];
console.log("spread call:", who.call(...spreadArgs));
