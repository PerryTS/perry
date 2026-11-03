// A [[Set]] (`o.k = v`, `this.k = v`) the compiler could fold into an object's birth runs an
// inherited setter once a prototype defines one; only a definition (a class field, a literal key) skips it.
const P: any = Object.prototype;
Object.defineProperty(P, "zz", { set(x: any) { (this as any).shadow = x; }, get() { return -1; }, configurable: true });
const o: any = {}; o.zz = 3;
console.log("lit-alias-zz", o.zz, Object.keys(o).join(","));
