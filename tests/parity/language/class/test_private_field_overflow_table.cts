// A private field keeps its storage when its holder gains an overflow table
// (a computed-key add, a delete, an accessor define): the table addresses its
// entries by position, a class layout stores its keys in another order, and
// the private read/write must follow the entry to the field's own slot.
const Z: string = String.fromCharCode(122);

// The original report: `#p` declared before a public field.
class Q0 { #p = 1; a = 2; bump(): number { this.#p += 1; return this.#p; } peek(): number { return this.#p; } }
{
  const q: any = new Q0();
  console.log(q.peek(), q.a);
  q[Z] = 1;
  console.log(q.peek(), q.a, Object.keys(q).join(","));
  q.a = 5;
  console.log(q.peek(), q.a);
  console.log(q.bump(), q.a, q.peek());
  const r: any = new Q0(); r.a = 9; console.log(r.bump(), r.a);
}

// Which step gives the holder a table.
class Q { #p = 1; a = 2; bump(): number { this.#p += 1; return this.#p; } peek(): number { return this.#p; } set(v: number) { this.#p = v; } has(o: any): boolean { return #p in o; } }
function show(tag: string, q: any) { console.log(tag, q.peek(), q.a, Object.keys(q).join(","), JSON.stringify(q)); }
{ const q: any = new Q(); show("base", q); }
{ const q: any = new Q(); q[Z] = 1; show("computed-add", q); q.set(7); show("computed-add set", q); console.log(q.bump(), q.a); }
{ const q: any = new Q(); q.zz = 1; show("fixed-add", q); q.set(7); show("fixed-add set", q); }
{ const q: any = new Q(); delete q.a; show("delete", q); q.set(7); show("delete set", q); q.a = 3; show("delete readd", q); }
{ const q: any = new Q(); Object.defineProperty(q, "a", { value: 4, writable: false, enumerable: true, configurable: true }); show("defprop", q); q.set(7); show("defprop set", q); }
{ const q: any = new Q(); Object.defineProperty(q, "g", { get() { return 11; }, enumerable: true, configurable: true }); show("accessor", q); q.set(7); show("accessor set", q); }
{ const q: any = new Q(); Object.freeze(q); show("freeze", q); q.set(7); show("freeze set", q); }
{ const q: any = new Q(); q[Z] = 1; console.log("in", q.has(q), q.has({}), q.has({ p: 1 }), "#p" in q, "p" in q, q["#p"], q.p); }
{ const q: any = new Q(); q["#p"] = "str"; q[Z] = 1; show("string #p key", q); q.set(8); show("string #p key set", q); console.log(q["#p"]); }
{ const q: any = new Q(); q[Z] = 1; q["#p"] = "str"; show("string #p key after", q); q.set(8); show("string #p key after set", q); console.log(q["#p"]); }
{ const q: any = new Q(); q[Z] = 1; for (const k in q) console.log("forin", k); console.log(Object.getOwnPropertyNames(q).join(","), Reflect.ownKeys(q).length, Object.entries(q).join(";")); const c = { ...q }; console.log(JSON.stringify(c)); }

// Field order, two private fields, private methods and accessors, static private, subclass.
class A { a = 2; #p = 1; peek(): number { return this.#p; } bump(): number { this.#p += 1; return this.#p; } }
{ const q: any = new A(); q[Z] = 1; console.log("after", q.peek(), q.a, q.bump(), q.a, Object.keys(q).join(",")); }
class B { #p = 1; #q = "two"; a = 3; b = 4; peek(): string { return this.#p + "/" + this.#q; } swap() { const t: any = this.#p; (this as any).#p = this.#q; (this as any).#q = t; } }
{ const q: any = new B(); console.log("two", q.peek(), q.a, q.b); q[Z] = 1; console.log("two", q.peek(), q.a, q.b); q.swap(); console.log("two", q.peek(), q.a, q.b, Object.keys(q).join(",")); }
class C { #p = 5; a = 6; #m(): number { return this.#p * 2; } get #g(): number { return this.#p + 100; } set #g(v: number) { this.#p = v; } run(): string { const x = this.#m(); this.#g = x; return x + ":" + this.#g + ":" + this.#p; } }
{ const q: any = new C(); q[Z] = 1; console.log("methods", q.run(), q.a, Object.keys(q).join(",")); }
class D { static #s = 10; static t = 20; static peek(): number { return D.#s; } static bump(): number { D.#s += 1; return D.#s; } }
{ const d: any = D; console.log("static", D.peek(), d.t); d[Z] = 1; console.log("static", D.peek(), d.t, D.bump(), d.t, D.peek()); }
class E { #p = 1; a = 2; peekE(): number { return this.#p; } setE(v: number) { this.#p = v; } }
class F extends E { #p = 30; b = 40; #r = 50; peekF(): string { return this.#p + "/" + this.#r; } setF(v: number) { this.#p = v; this.#r = v + 1; } }
{ const q: any = new F(); console.log("sub", q.peekE(), q.peekF(), q.a, q.b); q[Z] = 1; console.log("sub", q.peekE(), q.peekF(), q.a, q.b); q.setE(7); q.setF(8); console.log("sub", q.peekE(), q.peekF(), q.a, q.b, Object.keys(q).join(",")); }
class G { #p: any; a = 2; constructor(v: any) { this.#p = v; } peek(): any { return this.#p; } static eq(x: G, y: G): boolean { return x.#p === y.#p; } }
{ const x: any = new G("s"); const y: any = new G("s"); x[Z] = 1; console.log("cross", G.eq(x, y), x.peek(), y.peek(), x.a); delete x[Z]; console.log("cross", G.eq(x, y), x.peek(), x.a); }
{ const q: any = new G(1); q[Z] = 1; try { (E.prototype as any).peekE.call(q); console.log("brand: no throw"); } catch (e: any) { console.log("brand", e instanceof TypeError); } }
{ const arr: any[] = []; for (let i = 0; i < 4; i++) { const q: any = new G(i); if (i & 1) q[Z + i] = i; arr.push(q); } console.log("mixed", arr.map((q) => q.peek() + "." + q.a).join(" ")); }

// A private field stamped on an object that already has a table (return override), and one that gains it later.
class Ret { constructor(o: any) { return o; } }
class Stamp extends Ret { #s = 1; constructor(o: any) { super(o); } static get(o: any): any { return o.#s; } static set(o: any, v: any) { o.#s = v; } static has(o: any): boolean { return #s in o; } }
{
  const o: any = { a: 1, b: 2 }; o[Z] = 3; delete o.a;
  console.log("stamp", Stamp.has(o)); new Stamp(o);
  console.log("stamp", Stamp.has(o), Stamp.get(o), JSON.stringify(o)); Stamp.set(o, "x"); o.b = 20; o[Z + Z] = 4;
  console.log("stamp", Stamp.get(o), JSON.stringify(o), Object.keys(o).join(","));
  const u: any = { a: 1, b: 2 }; new Stamp(u); Stamp.set(u, 7); u[Z] = 3; delete u.a;
  console.log("stamp", Stamp.get(u), JSON.stringify(u)); Stamp.set(u, 8); u.b = 9;
  console.log("stamp", Stamp.get(u), JSON.stringify(u), Stamp.has(u), Stamp.has({}));
}
