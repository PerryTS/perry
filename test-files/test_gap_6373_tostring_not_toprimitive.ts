// #6373 — an explicit `x.toString()` is an ordinary [[Get]] + [[Call]] and must
// NEVER consult `[Symbol.toPrimitive]`. Only the ToString *coercion* paths
// (`String(x)`, `` `${x}` ``, `x + ""`) run ToPrimitive, which does honor
// `@@toPrimitive`. Before the fix, codegen's "universal `.toString()`" fold
// routed the method call through the coercion helper, so a receiver carrying an
// own `@@toPrimitive` had its `.toString()` hijacked by the symbol method.

// ── RegExp with own @@toPrimitive: .toString() ignores it, String() honors it ─
const re: any = /y/;
re[Symbol.toPrimitive] = () => "SYMPRIM";
console.log("re .toString():", re.toString()); // /y/  (RegExp.prototype.toString)
console.log("re String()  :", String(re)); // SYMPRIM  (ToPrimitive)
console.log("re template  :", `${re}`); // SYMPRIM
console.log('re "" + re   :', "" + re); // SYMPRIM

// ── Plain object: an own toString wins on the method call, @@toPrimitive on coercion ─
const o: any = { [Symbol.toPrimitive]: () => "SYM", toString: () => "TS" };
console.log("obj .toString():", o.toString()); // TS
console.log("obj String()  :", String(o)); // SYM
console.log("obj template  :", `${o}`); // SYM

// ── Only @@toPrimitive present: .toString() falls to Object.prototype.toString ─
const p: any = {};
p[Symbol.toPrimitive] = () => "PRIM";
console.log("prim .toString():", p.toString()); // [object Object]
console.log("prim String()  :", String(p)); // PRIM

// ── @@toPrimitive must not hijack .valueOf() either (Object.prototype.valueOf → this) ─
const v: any = {};
v[Symbol.toPrimitive] = () => "VP";
console.log("valueOf === self:", v.valueOf() === v); // true

// ── Regression: no @@toPrimitive still behaves ───────────────────────────────
const q: any = {};
console.log("plain .toString():", q.toString()); // [object Object]
const r: any = { toString: () => "OWN" };
console.log("own   .toString():", r.toString()); // OWN
console.log("own   String()  :", String(r)); // OWN
console.log("array .toString():", [1, 2, 3].toString()); // 1,2,3

// ── @@toPrimitive that returns a number: coercion stringifies it, method ignores it ─
const n: any = { [Symbol.toPrimitive]: () => 42 };
console.log("num  .toString():", n.toString()); // [object Object]
console.log("num  String()  :", String(n)); // 42
