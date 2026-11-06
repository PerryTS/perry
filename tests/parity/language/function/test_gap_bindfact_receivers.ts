// PLAN 3h: `f.bind(...)` on a base-shape function lowers to the builtin
// directly; every other receiver keeps the generic dispatch. No program
// write to Function.prototype here (the closed-world fact holds).

function add(this: any, a: number, b: number) { return (this ? this.k : 0) + a + b; }
const b1 = add.bind({ k: 1 }, 2);
console.log("bound", b1(3), b1.name, b1.length);
const b2 = b1.bind({ k: 100 }, 4);
console.log("bound of bound", b2(), b2.name, b2.length);

function Point(this: any, x: number, y: number) { this.x = x; this.y = y; }
const P1 = (Point as any).bind(null, 7);
const p = new P1(8);
console.log("new on bound", p.x, p.y, p instanceof (Point as any), p instanceof P1);

// A function with an own `bind`.
const own: any = function () { return 1; };
own.bind = function () { return "own bind"; };
console.log("own bind", own.bind({}));
// A function with an own key but no own `bind`: still the builtin.
const keyed: any = function (this: any) { return this.v; };
keyed.extra = 1;
console.log("keyed", keyed.bind({ v: 9 })());
// A function whose [[Prototype]] has another `bind`.
const reparented: any = function () { return 2; };
Object.setPrototypeOf(reparented, { bind() { return "proto bind"; } });
console.log("reparented", reparented.bind({}));
// Non-functions.
const objWithBind: any = { bind(x: any) { return "object bind " + x; } };
console.log(objWithBind.bind(5));
for (const v of [5, "s", {}, null, undefined] as any[]) {
  try { v.bind({}); console.log("no throw", typeof v); }
  catch (e: any) { console.log("throws", e instanceof TypeError); }
}
try { Function.prototype.bind.call({} as any); console.log("no throw"); }
catch (e: any) { console.log("bind.call(non-fn) throws", e instanceof TypeError); }

// A family constructor run of binds (ZodType's pattern), with an override.
class Base {
  tag: string;
  constructor() {
    this.tag = "base";
    this.hello = this.hello.bind(this);
    this.who = this.who.bind(this);
  }
  hello(): string { return "hello " + this.tag; }
  who(): string { return "base"; }
}
interface Base { hello: () => string; who: () => string; }
class Sub extends Base {
  constructor() { super(); this.tag = "sub"; }
  who(): string { return "sub"; }
}
for (let i = 0; i < 3; i++) {
  const b = new Base(), s = new Sub();
  const h = s.hello;
  console.log(b.hello(), b.who(), h(), s.who(), Object.keys(s).join(","));
}
