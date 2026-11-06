function Super(this: any, a: any) { this._a = a; }
(Super as any).extend = "EXT";
(Super as any).method = "METH";
(Super as any).MAX = 100;
(Super as any).prototype = { _a: null, path: "", initialize() {}, foo() { return 1; } };

const t: any = {};
Object.assign(t, Super);
if (t.extend !== "EXT" || t.method !== "METH" || t.MAX !== 100) throw new Error("enumerable statics missing");
const keys = Object.keys(t).sort();
if (JSON.stringify(keys) !== '["MAX","extend","method"]') throw new Error("keys: " + JSON.stringify(keys));
console.log("OK");
