// Own-key presence answered by the receiver's shape: every case where the key
// list, the shape's bound on it, or the receiver kind decides the answer.
const out: string[] = [];
function h(label: string, o: any, k: any): void {
  const a = Object.hasOwn(o, k);
  const b = Object.prototype.hasOwnProperty.call(o, k);
  let c: any = "n/a";
  try { c = o.hasOwnProperty(k); } catch (e) { c = "throws"; }
  out.push(label + " " + String(k) + " " + a + " " + b + " " + c);
}
function hit(o: any, k: string): boolean { return o.hasOwnProperty(k); }

// plain literal: atom keys, misses, runtime-built keys
const o: any = { a: 1, b: 2, c: 3 };
for (const k of ["a", "b", "c", "x", "", "ab", "toString", "hasOwnProperty"]) h("lit", o, k);
const built = ["a", "b", "c", "x"].map((s) => (s + "!").slice(0, 1));
for (const k of built) h("built", o, k);
h("long", { averyveryverylongpropertyname_1: 1 }, "averyveryverylongpropertyname_1");
h("long", { averyveryverylongpropertyname_1: 1 }, "averyveryverylongpropertyname_" + String(1));
h("long", { averyveryverylongpropertyname_1: 1 }, "averyveryverylongpropertyname_2");
h("uni", { "héllo": 1, "日本": 2 }, "héllo");
h("uni", { "héllo": 1, "日本": 2 }, "日" + "本");
h("uni", { "héllo": 1, "日本": 2 }, "hello");

// siblings sharing one canonical key list: the shape's count bounds membership
const p1: any = { a: 1, b: 2 };
const p2: any = { a: 1, b: 2 };
p2.c = 3;
p2.d = 4;
h("parent", p1, "c");
h("parent", p1, "d");
h("child", p2, "c");
h("child", p2, "d");
// add then check, delete then check, re-add
const g: any = { a: 1, b: 2 };
let seen = 0;
for (let i = 0; i < 2000; i++) if (hit(g, "c")) seen++;
g.c = 9;
for (let i = 0; i < 2000; i++) if (hit(g, "c")) seen++;
delete g.c;
for (let i = 0; i < 2000; i++) if (hit(g, "c")) seen++;
g.c = 10;
for (let i = 0; i < 2000; i++) if (hit(g, "c")) seen++;
delete g.a;
for (let i = 0; i < 2000; i++) if (hit(g, "a")) seen++;
out.push("add/delete seen " + seen);
h("deleted", g, "a");
h("deleted", g, "b");
h("deleted", g, "c");
// a delete of the LAST key (may move back to the parent shape)
const q: any = { a: 1, b: 2, c: 3 };
h("q", q, "c");
delete q.c;
h("q-del-last", q, "c");
h("q-del-last", q, "b");
q.d = 1;
h("q-add", q, "d");
h("q-add", q, "c");

// wide object: the shape's key index
const w: any = {};
for (let i = 0; i < 40; i++) w["k" + i] = i;
for (const k of ["k0", "k17", "k39", "k40", "k", "x"]) h("wide", w, k);
delete w.k17;
h("wide-del", w, "k17");
h("wide-del", w, "k18");

// numeric, symbol, nullish-ish keys
const n: any = { 0: "z", 1: "o", x: 1 };
h("num", n, "0");
h("num", n, 0);
h("num", n, 2);
const sym = Symbol("s");
const sy: any = { [sym]: 1, s: 2 };
h("sym", sy, sym);
h("sym", sy, "s");
h("undef", { undefined: 1 }, undefined);
h("null", { null: 1 }, null);
h("objkey", { "[object Object]": 1 }, {});

// receivers that are not plain literals
h("create-null", Object.assign(Object.create(null), { a: 1 }), "a");
h("create-null", Object.assign(Object.create(null), { a: 1 }), "b");
const fr: any = Object.freeze({ a: 1 });
h("frozen", fr, "a");
h("frozen", fr, "b");
const acc: any = { get a() { return 1; }, b: 2 };
h("accessor", acc, "a");
h("accessor", acc, "b");
h("accessor", acc, "c");
const js: any = JSON.parse('{"a":1,"b":{"c":2},"__proto__":3}');
h("json", js, "a");
h("json", js, "c");
h("json", js, "__proto__");
h("json-inner", js.b, "c");
h("literal-proto", { __proto__: null, a: 1 } as any, "__proto__");
class P { #secret = 1; pub = 2; m() { return this.#secret; } }
const pi: any = new P();
h("class", pi, "pub");
h("class", pi, "#secret");
h("class", pi, "m");
h("class-proto", P.prototype, "m");
h("class-proto", P.prototype, "constructor");
h("class-obj", P, "prototype");
h("class-obj", P, "name");
h("fn-proto", Function.prototype, "call");
h("fn-proto", Function.prototype, "hasOwnProperty");
h("fn-proto", Function.prototype, "valueOf");
h("obj-proto", Object.prototype, "toString");
h("obj-proto", Object.prototype, "hasOwnProperty");
h("obj-proto", Object.prototype, "call");
h("string-wrapper", new String("ab"), "0");
h("string-wrapper", new String("ab"), "2");
h("string-wrapper", new String("ab"), "length");
class A2 extends Array {}
const a2: any = new A2();
a2.push(5);
h("array-sub", a2, "0");
h("array-sub", a2, "length");
h("array-sub", a2, "1");
h("array", [1, 2], "0");
h("array", [1, 2], "length");
h("array", [1, 2], "2");
h("map", new Map(), "size");
h("env", process.env, "PATH");
h("env", process.env, "PERRY_DEFINITELY_NOT_SET_42");
(function (this: any, x: any) { h("arguments", arguments, "0"); h("arguments", arguments, "length"); h("arguments", arguments, "1"); })(1);
h("fn", function f(a: any, b: any) {}, "length");
h("fn", function f(a: any, b: any) {}, "prototype");
h("fn", () => 1, "prototype");
h("re", /x/g, "lastIndex");
h("re", /x/g, "source");
h("err", new Error("m"), "message");
h("err", new Error("m"), "stack");
h("date", new Date(0), "getTime");
// a replaced Object.prototype.hasOwnProperty is seen by o.hasOwnProperty
const saved = Object.prototype.hasOwnProperty;
(Object.prototype as any).hasOwnProperty = function () { return "replaced"; };
out.push("replaced " + String((o as any).hasOwnProperty("a")));
(Object.prototype as any).hasOwnProperty = saved;
out.push("restored " + String((o as any).hasOwnProperty("a")));
// own hasOwnProperty shadows
const sh: any = { hasOwnProperty() { return "own"; }, a: 1 };
out.push("shadow " + String(sh.hasOwnProperty("zz")) + " " + Object.hasOwn(sh, "a"));
// nullish receivers throw
try { Object.hasOwn(null as any, "a"); out.push("null no throw"); } catch (e: any) { out.push("null throws " + (e instanceof TypeError)); }
console.log(out.join("\n"));
