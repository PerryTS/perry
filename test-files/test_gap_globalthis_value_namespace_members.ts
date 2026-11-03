// globalThis used as a value reaches every global by a runtime key, so each
// namespace and web member table must be complete: no name below appears in
// the source as a key or string (they are spelled backwards), which used to
// leave Math, Reflect, Atomics and Intl born member-less and the text and
// WebSocket prototypes empty.
const g: any = globalThis;
const get = (reversed: string): any => g[reversed.split("").reverse().join("")];

const M = get("htaM");
console.log("Math", typeof M.abs, M.abs(-3), M.max(1, 7), typeof M.PI, Object.getOwnPropertyNames(M).length);
const R = get("tcelfeR");
console.log("Reflect", typeof R.ownKeys, R.ownKeys({ a: 1, b: 2 }).join(","), R.has({ x: 1 }, "x"));
const A = get("scimotA");
const ta = new Int32Array(new SharedArrayBuffer(8));
console.log("Atomics", typeof A.add, A.add(ta, 0, 5), A.load(ta, 0));
const I = get("ltnI");
console.log("Intl", typeof I.NumberFormat, Object.getOwnPropertyNames(I).includes("NumberFormat"));
const J = get("NOSJ");
console.log("JSON", J.stringify(J.parse("[1,2]")));
const TE = get("redocnEtxeT");
const TD = get("redoceDtxeT");
console.log("text", Array.from(new TE().encode("hi")).join(","), new TD().decode(new Uint8Array([104, 105])));
const WS = get("tekcoSbeW");
console.log("WebSocket", typeof WS.prototype.send, typeof WS.prototype.close);
