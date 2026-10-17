// computed keys: numeric-string, __proto__, symbols, slice/split/template keys, shape changes, for-in keys
const out: string[] = [];
const o: any = {};
const s = "a[b]=1&user[name]=x&tags[2]=c&__proto__=p&7=seven&constructor=k";
for (const part of s.split("&")) {
  const i = part.indexOf("=");
  const k = part.slice(0, i), v = part.slice(i + 1);
  if (k in o) out.push("dup " + k);
  o[k] = v;
}
out.push(Object.keys(o).join("|"));
out.push(String(o["7"]) + String(o[3 + 4]) + String(o["user[name]"]) + String(Object.getPrototypeOf(o) === Object.prototype));
const sym = Symbol("q"); o[sym] = 1; out.push(String(sym in o) + String(o[sym]));
for (let r = 0; r < 3; r++) {
  const k = ("key" + r).slice(0); o[k] = r; delete o[k]; o[k] = r * 2; out.push(k + ":" + o[k] + (k in o));
}
const t = `t${1}${"x".repeat(3)}`; o[t] = 1; o.t1xxx++; out.push(String(o[t]) + String(o["t1" + "xxx"]));
const p: any = { a: 1, b: 2, c: 3 }; const q: any = {};
for (const k in p) { q[k] = p[k] * 2; p[k + k] = 1; }
out.push(Object.keys(q).join() + Object.keys(p).join() + ("aa" in p) + ("zz" in p));
const big: any = {}; for (let i = 0; i < 300; i++) big["k" + i] = i;
let sum = 0; for (let i = 0; i < 300; i++) { const kk = ("zk" + i).slice(1); sum += big[kk]; }
out.push(String(sum) + String(("k299" in big)) + String(("k300" in big)));
const proto = { inh: 5 }; const ch: any = Object.create(proto); out.push(String(ch["in" + "h"]) + String("inh" in ch) + String(ch.hasOwnProperty("in" + "h")));
const an: any = {}; Object.defineProperty(an, "g", { get() { return 9; }, enumerable: true }); out.push(String(an[["g"].join("")]));
console.log(out.join("\n"));
