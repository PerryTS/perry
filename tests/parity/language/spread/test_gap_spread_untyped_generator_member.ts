// f(...x) where x has no class type (three's `_baseVector.set( ...target, 1 )` in
// SkinnedMesh.applyBoneTransform, `target` an untyped parameter): every class of the unit
// declaring a generator `*[Symbol.iterator]()` is a candidate behind its own family compare,
// and the drain reads the generator's `next` from %GeneratorPrototype% only while the
// member's own `.prototype` is unmaterialized. Compared against Node.
const log: string[] = [];
function f(...a: any[]) { return a.join(","); }
function g(a: any, b: any, c: any, d: any) { return [a, b, c, d].join(","); }

class Vec2 {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  *[Symbol.iterator]() { yield this.x; yield this.y; }
}
class Vec3 {
  x: number; y: number; z: number;
  constructor(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; }
  *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; }
}
class Vec3b extends Vec3 {}
class Vec3r extends Vec3 { *[Symbol.iterator]() { yield this.z; yield this.y; yield this.x; } }
class Out {
  x = 0; y = 0; z = 0; w = 0;
  set(x: any, y: any, z: any, w: any) { this.x = x; this.y = y; this.z = z; this.w = w; return this; }
}
const out = new Out();

// The untyped operand: a parameter, an `any` array element, an object property.
function apply(target: any) { out.set(...target, 1); return [out.x, out.y, out.z, out.w].join(","); }
const mixed: any[] = [new Vec3(1, 2, 3), new Vec2(4, 5), new Vec3b(6, 7, 8), new Vec3r(9, 10, 11), [12, 13], "ab", new Set([14])];
console.log("param", mixed.map(apply).join(" | "));
console.log("element", mixed.map((_, i) => f(...mixed[i])).join(" | "));
const bag: any = { v: new Vec3(3, 2, 1) };
console.log("property", g(...bag.v, 0), g(0, ...bag.v));
let s = 0;
for (let i = 0; i < 1000; i++) { (mixed[0] as Vec3).x = i; s += apply(mixed[i & 3]).length; }
console.log("loop", s);

// Non-iterables keep the TypeError; null and a number too.
class Plain { q = 1; }
for (const bad of [new Plain(), null, undefined, 5, {}]) {
  try { apply(bad); console.log("no-throw"); } catch (e) { console.log("noniter", e instanceof TypeError); }
}

// The member's own `.prototype` materialized and given an own `next`: the drain calls it.
class Gp { *[Symbol.iterator]() { yield 1; yield 2; } }
const gp: any = new Gp();
console.log("before-proto", apply(gp));
const member: any = (Gp.prototype as any)[Symbol.iterator];
const realNext = Object.getPrototypeOf(member.prototype).next;
member.prototype.next = function (this: any, ...a: any[]) {
  log.push("own-next");
  const r = realNext.apply(this, a);
  if (!r.done) r.value *= 10;
  return r;
};
console.log("own-next", apply(gp), log.join("|"));
delete member.prototype.next;
console.log("deleted-next", apply(gp));

// A getter for `next` on the member's prototype runs once per spread.
let reads = 0;
Object.defineProperty(member.prototype, "next", { get() { reads++; return realNext; }, configurable: true });
console.log("getter-next", apply(gp), reads);

// The generator object given an own `next` inside the member: own key wins.
class OwnNext {
  *[Symbol.iterator]() { yield "a"; yield "b"; }
}
const ownNext: any = new OwnNext();
const origMember = (OwnNext.prototype as any)[Symbol.iterator];
(OwnNext.prototype as any)[Symbol.iterator] = function (this: any) {
  const it = origMember.call(this);
  let n = 0;
  it.next = () => (n++ < 2 ? { value: "own" + n, done: false } : { value: undefined, done: true });
  return it;
};
console.log("own-key-next", f(...ownNext));

// The member replaced after instances exist, and an own @@iterator on one instance.
class Late { a = 1; *[Symbol.iterator]() { yield "old"; } }
const late: any = new Late();
console.log("late-before", f(...late));
(Late.prototype as any)[Symbol.iterator] = function* (this: any) { yield "new"; yield this.a; };
console.log("late-after", f(...late));
const one: any = new Vec3(1, 1, 1);
one[Symbol.iterator] = function* () { yield "own"; };
console.log("own-iter", apply(one), apply(new Vec3(2, 2, 2)));

// A throwing member body propagates; finally runs; an early return completes normally.
class Thrower { *[Symbol.iterator]() { try { yield 1; throw new Error("boom"); } finally { log.push("finally"); } } }
try { apply(new Thrower()); } catch (e) { console.log("throw", (e as Error).message, log.includes("finally")); }
class Early { *[Symbol.iterator]() { try { yield 1; return; } finally { log.push("early"); } } }
console.log("early", apply(new Early()), log.includes("early"));

// A generator that re-enters the same spread through its receiver.
class Reenter {
  depth = 0;
  *[Symbol.iterator]() { yield this.depth; if (this.depth++ < 2) yield f(...this); }
}
console.log("reenter", f(...(new Reenter() as any)));
console.log("done");
