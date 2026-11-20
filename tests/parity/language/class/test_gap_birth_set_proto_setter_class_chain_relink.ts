// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
class Base { set v(x: any) { (this as any).shadow = x; } get v(): any { return -1; } }
class Cell extends Base { declare v: number; constructor(v: number) { super(); this.v = v; } }
const a = new Cell(7);
console.log("class-proto-setter", (a as any).v, Object.keys(a).join(","));
class Plain { declare v: number; constructor(v: number) { this.v = v; } }
const pp: any = {}; Object.defineProperty(pp, "v", { set(x: any) { (this as any).shadow = x; }, get() { return -1; } });
Object.setPrototypeOf(Plain.prototype, pp);
const q = new Plain(7);
console.log("relinked-proto-setter", (q as any).v, Object.keys(q).join(","));
