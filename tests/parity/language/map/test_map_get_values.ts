// `Map.prototype.get` returns a value, never a key: the whole-program facts
// keep a Map's values apart from its keys while every write of its contents
// is a `set` they see, so a field read of an entry of a string-keyed Map is
// not the `undefined` a String's missing property reads, and a Number value
// stays a Number through typed calls. Every case prints what node prints;
// the negative controls are the Maps whose contents something else writes.
const many = process.argv.length > 100;

function use(m: number): number {
  return ((m & 127) * 3 + ((m >> 7) & 127)) | 0;
}

// (a) Entries of a string-keyed Map: the hit's field is a Number.
interface Entry { key: string; depth: number; move: number }
class Table {
  private table = new Map<string, Entry>();
  private hits = 0;
  probe(key: string, depth: number): number {
    const cached = this.table.get(key);
    let preferred = 0;
    if (cached !== undefined) {
      preferred = cached.move;
      if (cached.depth >= depth) this.hits++;
    }
    return use(preferred);
  }
  store(key: string, depth: number, move: number): void {
    if (this.table.size >= 40) this.table.clear();
    this.table.set(key, { key, depth, move });
  }
  walk(first: number, n: number): number {
    let move = first;
    let s = 0;
    for (let ply = 0; ply < n && move !== 0; ply++) {
      s = (s + use(move)) | 0;
      const next = this.table.get("k" + (move & 31));
      move = next === undefined ? 0 : next.move;
    }
    return s + this.hits;
  }
}
const t = new Table();
let acc = 0;
for (let i = 0; i < 300; i++) {
  const key = "k" + (i % 37);
  acc = (acc + t.probe(key, i & 7)) | 0;
  t.store(key, i & 7, (i * 131 + 7) & 16383);
}
console.log("entries", acc, t.walk(77, 12), t.walk(0, 3));

// (b) Number values under string keys, and string values under number keys.
const counts = new Map<string, number>();
function bump(k: string): number {
  const c = counts.get(k);
  const n = c === undefined ? 1 : c + 1;
  counts.set(k, n);
  return use(n);
}
let b = 0;
for (let i = 0; i < 50; i++) b += bump("w" + (i % 6));
console.log("counts", b, counts.size, counts.get("w0"), counts.get("nope"));
const names = new Map<number, string>();
for (let i = 0; i < 5; i++) names.set(i * 2, "n" + i);
function nameOf(k: number): string {
  const v = names.get(k);
  return v === undefined ? "-" : v + v.length;
}
console.log("names", nameOf(4), nameOf(3), nameOf(8));

// (c) A key is still a key for every other reader.
const kv = new Map<string, number>([["x", 1]]);
kv.set("y", 2);
let seen = "";
for (const [k, v] of kv) seen += k + v;
for (const k of kv.keys()) seen += k;
for (const v of kv.values()) seen += v;
kv.forEach((v, k) => (seen += k + v));
console.log("readers", seen, [...kv].length, kv.has("x"), kv.get("x"));

// ---- negative controls ----

// (d) A value that is a String (or anything) is what `get` returns.
const loose = new Map<string, any>();
loose.set("a", many ? 1 : "text");
loose.set("b", 7);
function looseAt(k: string): string {
  const v = loose.get(k);
  return v === undefined ? "none" : typeof v + ":" + (v.length === undefined ? use(v) : v.length);
}
console.log("loose", looseAt("a"), looseAt("b"), looseAt("c"));

// (e) A key that is the value too.
const self = new Map<any, any>();
self.set("k", "k");
self.set(5, 5);
console.log("self", self.get("k"), self.get(5), self.get("5"));

// (f) Spread arguments, `set` called through `call`/`apply` and `bind`.
const sp = new Map<string, any>();
const pair: [string, any] = ["p", "spread"];
sp.set(...pair);
Map.prototype.set.call(sp, "q", "called");
Map.prototype.set.apply(sp, ["r", "applied"]);
const bound = sp.set.bind(sp, "s");
bound("bound");
["u", "v"].forEach(sp.set, sp);
function spAt(k: string): string {
  const v = sp.get(k);
  return v === undefined ? "none" : String(v);
}
console.log("spread", spAt("p"), spAt("q"), spAt("r"), spAt("s"), spAt("u"), spAt("v"), spAt("w"));

// (g) A Map born from entries, from another Map, and a subclass.
const born = new Map<string, any>([["a", "one"], ["b", 2]]);
const copy = new Map(born);
copy.set("c", "three");
class Counted extends Map<string, any> {
  writes = 0;
  set(k: string, v: any): this {
    this.writes++;
    return super.set(k, k + v);
  }
}
const sub = new Counted();
sub.set("a", 1);
function anyAt(m: Map<string, any>, k: string): string {
  const v = m.get(k);
  return v === undefined ? "none" : typeof v + ":" + String(v);
}
console.log("born", anyAt(born, "a"), anyAt(born, "b"), anyAt(copy, "a"), anyAt(copy, "c"), anyAt(sub, "a"), sub.writes);

// (h) A Map that leaves the program's sight: a callback the runtime calls
// and a property bag hold it.
const shared = new Map<string, any>();
shared.set("n", 1);
const holder: any = { m: shared };
JSON.stringify(holder, (k, v) => {
  if (k === "m") v.set("late", "text");
  return k === "m" ? undefined : v;
});
(globalThis as any).__sharedMap = shared;
(globalThis as any).__sharedMap.set("global", "g");
console.log("shared", anyAt(shared, "n"), anyAt(shared, "late"), anyAt(shared, "global"));

// (i) `undefined` and a missing second argument are values.
const und = new Map<string, any>();
und.set("u", undefined);
(und as any).set("m");
console.log("undef", und.get("u"), und.get("m"), und.has("m"), und.size);

// (j) A WeakMap and a Set keep their own rules.
const wm = new WeakMap<object, any>();
const ko = { id: 1 };
wm.set(ko, "weak");
const st = new Set<any>(["a", 1]);
console.log("others", wm.get(ko), wm.get({}), st.has("a"), st.size);
