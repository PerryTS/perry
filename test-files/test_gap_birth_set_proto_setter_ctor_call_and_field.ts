// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
let n = 0; function noop() { n++; }
class Cell { declare v: number; constructor(v: number) { this.v = v; noop(); } }
class Cell2 { v: number; constructor(v: number) { this.v = v; } }
const b = new Cell(1); const b2 = new Cell2(1);
const P: any = Object.prototype;
Object.defineProperty(P, "v", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true });
const a = new Cell(7);
console.log("ctor-call", (a as any).v, Object.keys(a).join(","));
const a2 = new Cell2(7);
console.log("field-define", (a2 as any).v, Object.keys(a2).join(","));
const o: any = { a: 1 }; noop(); o.v = 5;
console.log("lit-store-after-call", o.v, Object.keys(o).join(","));
