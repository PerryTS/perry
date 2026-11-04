// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
Object.defineProperty(Object.prototype, "v", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true });
const o: any = {}; o.v = 3;
console.log("lit-direct", o.v, Object.keys(o).join(","));
function put(r: any, x: number) { r.v = x; }
const p: any = {}; put(p, 4);
console.log("lit-fn", p.v, Object.keys(p).join(","));
const q: any = { a: 1 }; q.v = 5;
console.log("lit-a", q.v, Object.keys(q).join(","));
const k = "v"; const r: any = {}; r[k] = 6;
console.log("lit-keyed", r.v, Object.keys(r).join(","));
