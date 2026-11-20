// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
// DEC 250: Object.prototype takes no new key in a compiled program, so the setter is refused and the store is an own data property.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const P: any = Object.prototype;
locked(() => { Object.defineProperty(P, "v", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true }); });
const o: any = {}; o.v = 3;
console.log("lit-alias", o.v, Object.keys(o).join(","));
function put(r: any, x: number) { r.v = x; }
const p: any = {}; put(p, 4);
console.log("lit-fn", p.v, Object.keys(p).join(","));
const k = "v"; const r: any = {}; r[k] = 6;
console.log("lit-keyed", r.v, Object.keys(r).join(","));
