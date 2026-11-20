// A function-constructor, replaced or Object.create prototype that defines
// `toLocaleString` wins over Object.prototype's on every call form: direct,
// via `any`, and computed. Object.prototype's default still applies to an
// object whose chain defines none.
function F(this: any) { this.v = 1; }
F.prototype.toLocaleString = function (...a: any[]) { return "F/" + a.length; };
const f: any = new (F as any)();
const k = "toLocaleString";
console.log(f.toLocaleString(), f[k](), f.toLocaleString.call(f));
function G(this: any) {}
G.prototype = { toLocaleString() { return "G"; } };
const g: any = new (G as any)();
const base: any = { toLocaleString() { return "base"; } };
const oc: any = Object.create(base);
function via(x: any) { return x.toLocaleString(); }
console.log(g.toLocaleString(), via(g), oc.toLocaleString(), via(oc), via(f));
const plain: any = { a: 1 };
console.log(plain.toLocaleString(), via(plain), via([1, 2]), via(5));
const nullProto: any = Object.create(null);
try { nullProto.toLocaleString(); console.log("null proto: missed"); }
catch (e) { console.log("null proto:", e instanceof TypeError); }
