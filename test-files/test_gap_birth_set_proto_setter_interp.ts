// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
class Cell { declare v: number; constructor(v: number) { this.v = v; } }
const b = new Cell(1);
const install = new Function("o", "Object.defineProperty(Object.getPrototypeOf(o), \"v\", { set(x) { this.shadow = x; }, get() { return -1; }, configurable: true });");
install({});
const a = new Cell(7);
console.log("interp-class", (a as any).v, Object.keys(a).join(","));
const o: any = {}; o.v = 3;
console.log("interp-literal", o.v, Object.keys(o).join(","));
function mk(v: number): any { const r: any = {}; r.v = v; return r; }
const m = mk(4);
console.log("interp-fnobj", m.v, Object.keys(m).join(","));
