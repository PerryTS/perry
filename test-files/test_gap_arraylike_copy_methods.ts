// Array.prototype.{flatMap,flat,toReversed,toSorted,with,keys,entries} on a
// Proxy over an Array (a Solid store array) and on a plain array-like object.
// They answered `{}` / `[]` / "flatMap is not a function"; opencode's session
// view crashed on `.length` of the undefined that followed.
const log: string[] = [];
const traced = (target: any[]) =>
  new Proxy(target, {
    get(t, k, r) { if (typeof k === "string" && /^\d+$/.test(k)) log.push("get" + k); return Reflect.get(t, k, r); },
    has(t, k) { if (typeof k === "string") log.push("has" + k); return Reflect.has(t, k); },
  });
const show = (label: string, f: () => any) => {
  log.length = 0;
  try { console.log(label, JSON.stringify(f()), log.join(",")); } catch (e: any) { console.log(label, "THROW", e.constructor.name, e.message); }
};
const p: any = traced([3, 1, 2]);
show("flatMap", () => p.flatMap((x: number, i: number) => (i === 1 ? [x, [x]] : x)));
show("flat", () => traced([[1], 2, [[3]]] as any).flat());
show("toReversed", () => p.toReversed());
show("toSorted", () => p.toSorted());
show("toSorted cmp", () => p.toSorted((a: number, b: number) => b - a));
show("with", () => p.with(-1, 9));
show("with range", () => p.with(3, 9));
show("keys", () => [...p.keys()]);
show("entries", () => [...p.entries()]);
show("values", () => [...p.values()]);
const store: any = new Proxy([{ parts: [1, 2] }, { parts: [3] }], {});
show("store flatMap", () => store.flatMap((m: any) => m.parts).length);
const A: any = Array.prototype;
const o: any = { length: 3, 0: "c", 2: "a" };
show("call flatMap", () => A.flatMap.call(o, (x: any) => [x]));
show("call toReversed", () => A.toReversed.call(o));
show("call toSorted", () => A.toSorted.call(o));
show("call with", () => A.with.call(o, 1, "b"));
show("call keys", () => [...A.keys.call(o)]);
show("call entries", () => [...A.entries.call(o)]);
show("call flatMap bad", () => A.flatMap.call(o, 1));
show("lengths", () => [A.flatMap.length, A.toSorted.length, A.with.length, A.keys.length]);
show("real array", () => [[1, [2]].flatMap((x: any) => x), [2, 1].toSorted(), [1, 2].with(0, 5), [...["a"].entries()]]);
