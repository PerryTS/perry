// A compound / logical member assignment used as a value evaluates its base
// and computed key once (the statement form was #6071).
const log: string[] = [];
const levels: Set<string>[] = [];
let depth = 0;
(levels[depth++] ??= new Set()).add("a");
(levels[depth++] ??= new Set()).add("b");
(levels[0] ??= new Set()).add("c");
console.log("??=", depth, levels.length, levels.map((s) => [...s].join("")).join("|"));

const arr = [1, 2, 3, 4];
let k = 0;
const v1 = (arr[k++] += 10);
const v2 = (arr[k++] *= 3);
const v3 = (arr[k++] -= 1);
console.log("arith", k, JSON.stringify(arr), v1, v2, v3);

const box = { n: 1 };
const get = () => (log.push("base"), box);
const v4 = (get().n += 5);
const v5 = (get().n ||= 99);
const v6 = (get().n &&= 7);
console.log("base", log.join(","), box.n, v4, v5, v6);

log.length = 0;
const keyed: Record<string, number | undefined> = { a: 0 };
const key = (s: string) => (log.push("key:" + s), s);
const v7 = (keyed[key("a")] ||= 4);
const v8 = (keyed[key("a")] ||= 5);
const v9 = (keyed[key("b")] ??= 6);
console.log("key", log.join(","), JSON.stringify(keyed), v7, v8, v9);

log.length = 0;
const target = {
  _v: 1,
  get v() { log.push("get"); return this._v; },
  set v(x: number) { log.push("set:" + x); this._v = x; },
};
const holder = () => (log.push("holder"), target);
const v10 = (holder().v += 1);
const v11 = (holder().v ||= 50);
const v12 = (holder().v &&= 0);
console.log("accessors", log.join(","), target._v, v10, v11, v12);

const plain = { count: 0 };
const v13 = (plain.count += 2);
const table = [0, 0];
let i = 1;
const v14 = (table[i] += 3);
console.log("plain", plain.count, v13, JSON.stringify(table), v14);
