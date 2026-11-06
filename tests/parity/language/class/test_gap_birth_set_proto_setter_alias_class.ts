// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
class Cell { declare v: number; constructor(v: number) { this.v = v; } }
const b = new Cell(1);
const P: any = Object.prototype;
Object.defineProperty(P, "v", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true });
const a = new Cell(7);
console.log("compiled", (a as any).v, Object.keys(a).join(","));
