// Class constructors as function objects: printing, statics blocks, private
// statics, super in statics, metadata-free reflection, collections.
class A { static #count = 0; static inc() { return ++A.#count; } static { (A as any).boot = "booted"; } }
class B extends A { static who() { return super.inc() * 10; } }
console.log(A, B, [A, 1], { k: B });
console.log(A.inc(), B.who(), (A as any).boot, (B as any).boot);
class H { static [Symbol.hasInstance](v: any) { return v === 42; } }
console.log((42 as any) instanceof H, ({} as any) instanceof H);
const byClass = new Map<Function, number>();
for (const C of [A, B, A, H]) byClass.set(C, (byClass.get(C) ?? 0) + 1);
console.log([...byClass.values()].join(","), byClass.has(A), byClass.has(1 as any));
const ws = new WeakSet<object>([A, B]);
console.log(ws.has(A), ws.has(H));
const wr = new WeakRef(A);
console.log(wr.deref() === A);
const obj: Record<string, any> = {};
obj[A.name] = A;
console.log(obj.A === A, typeof obj.A);
const arr = [H, B, A];
arr.sort((x: any, y: any) => x.name.localeCompare(y.name));
console.log(arr.map((c) => c.name).join(","));
console.log([A, B].indexOf(B), [1, 2, 3].indexOf(A as any), [A].lastIndexOf(A));
function takesCtor(c: new () => object) { return new c(); }
console.log(takesCtor(B) instanceof A);
class P { v: string; constructor() { this.v = new.target.name; } }
class Q extends P {}
console.log(new P().v, new Q().v);
const bound = (B as any).who.bind(B);
console.log(bound());
let n: any = A;
n = n === A ? "same" : "diff";
console.log(n, (A as any) == (A as any), (A as any) === (B as any));
console.log(Number.isInteger(A as any), Array.isArray(A), typeof (A as any).prototype);
console.log(String([A]).startsWith("class A"), `${[B]}`.includes("extends A"));
