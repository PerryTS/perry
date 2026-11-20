// Object literals with duplicate keys: one own key, at the first
// occurrence position, holding the last definition. Values still run
// in source order.
const log: string[] = [];
const t = <T>(tag: string, v: T): T => { log.push(tag); return v; };
const show = (label: string, o: any) => {
  const parts = Reflect.ownKeys(o).map((k) => {
    const d = Object.getOwnPropertyDescriptor(o, k)!;
    const kind = "value" in d ? JSON.stringify(d.value) ?? String(d.value) : "acc(" + (d.get ? "g" : "-") + (d.set ? "s" : "-") + ")";
    return String(k) + "=" + kind;
  });
  console.log(label, parts.join(" "), "|", log.splice(0).join(","));
};

show("kv2", { x: 1, x: 2 });
show("kv3", { x: 1, y: 2, x: 3 });
show("order", { x: t("x1", 1), y: t("y", 2), x: t("x2", 3), z: t("z", 4) });
const a = 10;
show("short1", { a, a: 2 });
show("short2", { a: 1, b: 2, a });
show("num", { 1: "n", "1": "s", 2: 0, 1.0: "f" });
show("meth1", { m() { return 1; }, n: 0, m() { return 2; } });
const mm = { m() { return 1; }, m() { return 2; } };
console.log("meth1 call", mm.m());
show("meth2", { m: 1, k: 2, m() { return 3; } });
show("meth3", { m() { return 3; }, k: 2, m: 1 });
const g1: any = { x: 1, get x() { return 2; } };
show("get-after-data", g1);
console.log("g1.x", g1.x);
const g2: any = { get x() { return 1; }, y: 0, x: 2 };
show("data-after-get", g2);
const g3: any = { get x() { return 1; }, y: 0, set x(v: number) { log.push("set" + v); } };
show("get+set", g3);
g3.x = 5;
console.log("g3.x", g3.x, log.splice(0).join(","));
const g4: any = { get x() { return 1; }, x: 2, set x(v: number) {} };
show("get,data,set", g4);
console.log("g4.x", g4.x);
const g5: any = { set x(v: number) {}, get x() { return 7; }, x: 8, get x() { return 9; } };
show("set,get,data,get", g5);
const k = "x";
const sym = Symbol("s");
show("comp1", { x: 1, [k]: 2 });
show("comp2", { [k]: 1, y: 0, x: 2 });
show("comp3", { x: t("x", 1), [t("k", k)]: t("v", 2), y: t("y", 3) });
show("comp4", { [sym]: 1, a: 0, [sym]: 2 });
show("comp5", { ["x"]: 1, x: 2, ["y"]: 3, y: 4 });
const src = { x: 1, y: 2 };
show("spread1", { ...src, x: 3 });
show("spread2", { x: 0, ...src });
show("spread3", { x: 0, ...src, x: 3, w: 4 });
show("spread4", { y: 0, ...{ y: 9, q: 1 }, q: 2 });
const p = { inherited: true };
const pr: any = { ["__proto__"]: 1, __proto__: p, ["__proto__"]: 2 };
show("proto1", pr);
console.log("proto1 inh", Object.getPrototypeOf(pr) === p);
const __proto__ = 5;
const pr2: any = { __proto__, a: 1, __proto__: p };
show("proto2", pr2);
console.log("proto2 inh", Object.getPrototypeOf(pr2) === p);
show("proto3", { __proto__() { return 1; }, __proto__: null } as any);
function make(v: number) { return { kind: "a", v, kind: "b" }; }
show("fn1", make(1));
show("fn2", make(2));
for (let i = 0; i < 3; i++) {
  const o = { i, j: i * 2, i: i + 100 };
  show("loop" + i, o);
}
const nested = { a: { x: 1, x: 2 }, a: { y: 1, y: 3 } };
show("nested", nested);
show("nested.a", nested.a);
console.log(JSON.stringify({ x: 1, y: 2, x: 3 }), Object.keys({ b: 1, a: 2, b: 3 }).length);
for (const key in { p: 1, q: 2, p: 3 }) log.push(key);
console.log("forin", log.splice(0).join(","));
const big = { a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10, a: 11, k: 12 };
show("big", big);
console.log("big.a", big.a, big.k, Object.values(big).length);
