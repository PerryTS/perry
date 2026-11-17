// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
// DEC 250: Object.prototype takes no new key in a compiled program, so the setter is refused and the store is an own data property.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const P: any = Object.prototype;
locked(() => { Object.defineProperty(P, "zz", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true }); });
const o: any = {}; o.zz = 3;
console.log("lit-alias-zz", o.zz, Object.keys(o).join(","));
