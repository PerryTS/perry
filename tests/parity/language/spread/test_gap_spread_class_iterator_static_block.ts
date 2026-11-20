// three.js's vector classes write their prototype in a static block (`static {
// Vector3.prototype.isVector3 = true; }`), so a spread of an instance (`_baseVector.set(
// ...target, 1 )`) cannot use the compiler's static-shape proof and takes the runtime drain.
// There, a compiled class instance with no own @@iterator and no recorded prototype whose
// declared prototype chain holds a generator function as a data property is spread through
// that method directly (symbol::class_instance_generator_symbol_method). Every other receiver
// keeps the complete GetIterator. Compared against Node.
const log: string[] = [];
function f(...a: any[]) { return a.join(","); }
class Vec3 {
  static { (Vec3.prototype as any).isVec3 = true; }
  x: number; y: number; z: number;
  constructor(x: number, y: number, z: number) { this.x = x; this.y = y; this.z = z; }
  *[Symbol.iterator]() { yield this.x; yield this.y; yield this.z; }
}
class Vec3b extends Vec3 { static { (Vec3b.prototype as any).isVec3b = true; } }
class Vec3r extends Vec3 { *[Symbol.iterator]() { yield this.z; yield this.x; } }
const typed: Vec3 = new Vec3(1, 2, 3);
const loose: any[] = [typed, new Vec3b(4, 5, 6), new Vec3r(7, 8, 9)];
console.log("typed", f(...typed), "loose", loose.map((v) => f(...v)).join(" | "));
let s = 0;
for (let i = 0; i < 2000; i++) { typed.x = i; s += f(...loose[i % 3], i).length; }
console.log("loop", s);

// A getter on a subclass prototype shadows the parent's generator; it runs once per spread.
class Shadow extends Vec3 {}
Object.defineProperty(Shadow.prototype, Symbol.iterator, {
  get() { log.push("get"); return function* (this: any) { yield "shadow"; yield this.x; }; },
  configurable: true,
});
console.log("getter", f(...(new Shadow(1, 2, 3) as any)), log.length);

// A plain (non-generator) method on the prototype, and a native one (Array.prototype.values).
class Plain { a = 1; [Symbol.iterator]() { let i = 0; return { next: () => (i++ < 2 ? { value: "p" + i, done: false } : { value: undefined, done: true }) }; } }
console.log("plain-method", f(...(new Plain() as any)));
class Native { 0 = "n0"; 1 = "n1"; length = 2; }
(Native.prototype as any)[Symbol.iterator] = Array.prototype.values;
console.log("native-method", f(...(new Native() as any)));

// Own @@iterator on an instance, a recorded prototype, a frozen instance, a deleted member.
const own: any = new Vec3(1, 1, 1);
own[Symbol.iterator] = function* () { yield "own"; };
const relinked: any = Object.setPrototypeOf(new Vec3(2, 2, 2), { *[Symbol.iterator]() { yield "relinked"; } });
const frozen = Object.freeze(new Vec3(3, 3, 3));
console.log("own", f(...own), "relinked", f(...relinked), "frozen", f(...(frozen as any)));
class Gone { *[Symbol.iterator]() { yield 1; } }
const gone: any = new Gone();
delete (Gone.prototype as any)[Symbol.iterator];
try { f(...gone); console.log("gone no-throw"); } catch (e) { console.log("gone", e instanceof TypeError); }

// Builtin subclasses with a declared generator member: the member drives the spread.
class MyArr extends Array { *[Symbol.iterator]() { yield "arr-member"; } }
class MyMap extends Map<any, any> { *[Symbol.iterator]() { yield "map-member"; } }
class MySet extends Set<any> { *[Symbol.iterator]() { yield "set-member"; } }
const arr = new MyArr(); arr.push(1, 2);
console.log("builtin-subclass", f(...arr), f(...new MyMap([[1, 2]])), f(...new MySet([1])));
// ...and without one: the builtin iteration.
class PlainArr extends Array {}
class PlainMap extends Map<any, any> {}
const parr = new PlainArr(); parr.push(3, 4);
console.log("builtin-default", f(...parr), f(...new PlainMap([[5, 6]]).keys()), [...new PlainMap([[7, 8]])].length);

// The generator throws, returns early, and re-enters a spread of the same receiver.
class Thrower { static { (Thrower.prototype as any).k = 1; } *[Symbol.iterator]() { try { yield 1; throw new Error("boom"); } finally { log.push("fin"); } } }
try { f(...(new Thrower() as any)); } catch (e) { console.log("throw", (e as Error).message, log.includes("fin")); }
class Reenter { static { (Reenter.prototype as any).k = 1; } d = 0; *[Symbol.iterator](): any { yield this.d; if (this.d++ < 2) yield f(...this); } }
console.log("reenter", f(...(new Reenter() as any)));
// The member's own prototype given a `next`.
const member: any = (Vec3.prototype as any)[Symbol.iterator];
const realNext = Object.getPrototypeOf(member.prototype).next;
member.prototype.next = function (this: any) { const r = realNext.call(this); if (!r.done) r.value = -r.value; return r; };
console.log("member-next", f(...typed), f(...loose[1]));
console.log("done");
