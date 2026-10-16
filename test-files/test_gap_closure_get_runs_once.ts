// A function object read: the own lookup and the %Function.prototype% walk run
// once per read, and every key the by-name path synthesizes for functions
// still resolves (caller/arguments poison pill, prototype, constructor,
// length, name).
function isBuffer(obj: any): boolean {
  if (!obj || typeof obj !== "object") return false;
  return !!(obj.constructor && obj.constructor.isBuffer && obj.constructor.isBuffer(obj));
}
console.log(isBuffer({}), isBuffer([]), isBuffer(new Date(0)), isBuffer(Buffer.from("a")));

const any = (x: any) => x;
function plain(a: number, b: number) { return a + b; }
const f: any = plain;
console.log(f.missing, any(Object).isBuffer, any(Array).nope, any(Math.max).absent);
console.log(typeof f.call, typeof f.apply, typeof f.bind, typeof f.toString, typeof f.hasOwnProperty);
console.log(f.length, f.name, typeof f.prototype, f.constructor === Function);
try { any(plain).caller; console.log("no throw"); } catch (e: any) { console.log("caller throws", e instanceof TypeError); }
try { any(plain).arguments; console.log("no throw"); } catch (e: any) { console.log("arguments throws", e instanceof TypeError); }

// An own getter returning undefined runs exactly once per read.
let calls = 0;
const g: any = function g() {};
Object.defineProperty(g, "lazy", { get() { calls++; return undefined; }, configurable: true });
const v1 = g.lazy; const v2 = g.lazy;
console.log(v1, v2, calls);

// An own data property holding undefined, and an expando on Function.prototype.
const h: any = function h() {};
h.u = undefined;
(Function.prototype as any).shared = 42;
console.log(h.u, "u" in h, h.shared, any(Object).shared, h.notThere);
delete (Function.prototype as any).shared;
console.log(h.shared, any(Object).shared);

// Deleted own name/length fall back to Function.prototype.
const k: any = function kk(a: number) { return a; };
delete k.name; delete k.length;
console.log(JSON.stringify(k.name), k.length);

// Class constructors keep their own lookup.
class Base { static s = 1; static get gs() { return 2; } }
class Derived extends Base {}
const D: any = Derived;
console.log(D.s, D.gs, D.missing, typeof D.prototype, D.name, D.length);

// A bound function and an arrow.
const bound: any = plain.bind(null, 1);
const arrow: any = () => 1;
console.log(bound.name, bound.length, bound.missing, arrow.prototype, arrow.missing);
// A setPrototypeOf chain on a function.
const proto = { tag: "T" };
const withProto: any = function wp() {};
Object.setPrototypeOf(withProto, proto);
console.log(withProto.tag, withProto.missing);
