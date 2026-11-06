const s1 = Symbol("s1");
const s2 = Symbol("s2");
function mk(): any {
  const o: any = { a: 1 };
  o[s1] = 10;
  o.b = 2;
  o[s2] = 20;
  o.c = 3;
  return o;
}
const o = mk();
console.log("keys", JSON.stringify(Object.keys(o)));
console.log("names", JSON.stringify(Object.getOwnPropertyNames(o)));
console.log("ownKeys", Reflect.ownKeys(o).map(String).join(","));
console.log("syms", Object.getOwnPropertySymbols(o).map(String).join(","));
console.log("entries", JSON.stringify(Object.entries(o)));
console.log("values", JSON.stringify(Object.values(o)));
const fi: string[] = [];
for (const k in o) fi.push(k);
console.log("forin", fi.join(","));
console.log("json", JSON.stringify(o));
const as: any = Object.assign({}, o);
console.log("assign", as[s1], as[s2], Reflect.ownKeys(as).map(String).join(","));
const sp: any = { ...o };
console.log("spread", sp[s1], sp[s2], Reflect.ownKeys(sp).map(String).join(","));
const sc: any = structuredClone(o);
console.log("clone", sc[s1], Reflect.ownKeys(sc).map(String).join(","));
console.log("in", "a" in o, s1 in o, "s1" in o, o.hasOwnProperty("b"), o.hasOwnProperty(s2));
delete o.b;
console.log("afterdel", Reflect.ownKeys(o).map(String).join(","), o.c, o[s2]);
delete o[s1];
console.log("afterdelsym", Reflect.ownKeys(o).map(String).join(","), o.a, o.c, o[s2]);
o[s1] = 11;
console.log("readd", o[s1], Reflect.ownKeys(o).map(String).join(","));
const big: any = {};
for (let i = 0; i < 20; i++) { big["k" + i] = i; if (i % 5 === 0) big[Symbol.for("g" + i)] = -i; }
console.log("big", Object.keys(big).length, Object.getOwnPropertySymbols(big).length, big.k19, big[Symbol.for("g15")]);
Object.freeze(o);
console.log("frozen", Object.isFrozen(o), Object.getOwnPropertyDescriptor(o, s2)!.writable);
class C { x = 1; [s1] = 5; y = 2; }
const c = new C();
console.log("class", JSON.stringify(c), Reflect.ownKeys(c).map(String).join(","), c[s1]);
const d = Object.getOwnPropertyDescriptors(o);
console.log("descs", Reflect.ownKeys(d).map(String).join(","));
const e: any = {};
Object.defineProperty(e, s1, { get() { return 99; }, enumerable: false });
e.z = 1;
console.log("acc", e[s1], JSON.stringify(Object.keys(e)), Object.getOwnPropertySymbols(e).length);
const m = new Map<any, number>([[o, 1]]);
console.log("map", m.get(o));
const iter: any = { [Symbol.iterator]: function* () { yield 1; yield 2; }, n: 1 };
console.log("iter", [...iter].join(","), Object.keys(iter).join(","));
const tag: any = { [Symbol.toStringTag]: "Tagged", q: 1 };
console.log("tag", String(tag), Object.prototype.toString.call(tag));
function rd(x: any) { return x[s2]; }
let acc = 0; const objs = [mk(), mk(), { [s2]: 7 }, mk()];
for (let i = 0; i < 1000; i++) acc += rd(objs[i & 3]);
console.log("ic", acc);
