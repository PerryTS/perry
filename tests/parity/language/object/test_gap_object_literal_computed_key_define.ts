// A computed object-literal key is a definition (CreateDataPropertyOrThrow),
// never [[Set]]: a key evaluating to "__proto__" is an own data property and
// an Object.prototype accessor for the key does not run.
// DEC 250: Object.prototype takes no new key in a compiled program, so the two accessors are refused; the computed keys still define own properties.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
const key = "__proto__";
const a: any = { [key]: 99 };
console.log("a", Object.prototype.hasOwnProperty.call(a, "__proto__"), Object.getPrototypeOf(a) === Object.prototype, JSON.stringify(Object.getOwnPropertyDescriptor(a, "__proto__")));
const p = { inh: 1 };
const c: any = { ["__proto__"]: 1, __proto__: p, ["__proto__"]: 2 };
console.log("c", Object.keys(c).join(","), c.inh, Object.getPrototypeOf(c) === p, c.__proto__ === p);
function mk(k: string) { return { [k]: 5, z: 1 }; }
const d: any = mk("__proto__");
console.log("d", Object.keys(d).join(","), Object.getPrototypeOf(d) === Object.prototype);
let ran = 0;
locked(() => { Object.defineProperty(Object.prototype, "trap", { set(_v) { ran++; }, get() { return "inherited"; }, configurable: true }); });
const name = "trap";
const e: any = { [name]: 7 };
console.log("e", ran, e.trap, Object.keys(e).join(","));
const sym = Symbol("s");
locked(() => { Object.defineProperty(Object.prototype, sym, { set(_v) { ran++; }, configurable: true }); });
const f: any = { [sym]: 8 };
console.log("f", ran, f[sym], Object.getOwnPropertySymbols(f).length);
locked(() => { delete (Object.prototype as any).trap; });
locked(() => { delete (Object.prototype as any)[sym]; });
