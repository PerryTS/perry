// #2905 — global builtin function values on globalThis
console.log(typeof globalThis.parseInt);
console.log(typeof globalThis.parseFloat);
console.log(typeof globalThis.isNaN);
console.log(typeof globalThis.isFinite);
console.log(typeof globalThis.encodeURIComponent);
console.log(typeof globalThis.decodeURIComponent);

const p = globalThis.parseInt;
console.log(p("42px"));
console.log(p("ff", 16));

const pf = globalThis.parseFloat;
console.log(pf("3.14xyz"));

const nan = globalThis.isNaN;
console.log(nan(NaN), nan(3));

const fin = globalThis.isFinite;
console.log(fin(3), fin(Infinity));

const enc = globalThis.encodeURIComponent;
const dec = globalThis.decodeURIComponent;
console.log(enc("a b?"));
console.log(dec("a%20b%3F"));

// bare-binding rebinding
const p2 = parseInt;
console.log(p2("100", 2));

// #2889 — rebound global builtin constructors through real semantics
const A = Array;
console.log(new A(2).length);
console.log(Array.isArray(new A(1)));

const O = Object;
console.log(O.keys({ a: 1, b: 2 }).join(","));

const N = Number;
console.log(N("42"));

const S = String;
console.log(S(123));

const B = Boolean;
console.log(B(0), B(1));
