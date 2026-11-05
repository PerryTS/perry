// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
class Cell { declare v: number; constructor(v: number) { this.v = v; } }
const install = new Function("o", "Object.defineProperty(Object.getPrototypeOf(o), \"v\", { set(x) { this.shadow = x; }, get() { return -1; }, configurable: true });");
install({});
const a = new Cell(7);
console.log("interp-class-nowarm", (a as any).v, Object.keys(a).join(","));
