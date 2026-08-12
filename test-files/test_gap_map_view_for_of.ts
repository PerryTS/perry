// Gap test: `for (… of m.values() / m.keys() / m.entries())` and single-ident
// Map/Set for-of heads.
//
// These loops are lowered to a direct index walk over the collection's flat
// entries buffer instead of the generic iterator protocol. The walk must be
// indistinguishable from the protocol it replaces: same insertion order, same
// live view of a body that mutates the collection mid-iteration, same fresh
// `[key, value]` pair per step, and the shapes the walk cannot express must
// keep the protocol.
//
// Run: node --experimental-strip-types test_gap_map_view_for_of.ts

function line(label: string, value: unknown): void {
  console.log(label + ": " + String(value));
}

function mkMap(n: number): Map<string, number> {
  const m = new Map<string, number>();
  for (let i = 0; i < n; i++) m.set("k" + i, i);
  return m;
}

// --- values() / keys() / entries(): plain sweeps ---
{
  const m = mkMap(5);
  const v: number[] = [];
  for (const x of m.values()) v.push(x);
  line("values", v.join(","));

  const k: string[] = [];
  for (const x of m.keys()) k.push(x);
  line("keys", k.join(","));

  const kv: string[] = [];
  for (const [a, b] of m.entries()) kv.push(a + "=" + b);
  line("entries.kv", kv.join(","));

  const only_k: string[] = [];
  for (const [a] of m.entries()) only_k.push(a);
  line("entries.k", only_k.join(","));

  const only_v: number[] = [];
  for (const [, b] of m.entries()) only_v.push(b);
  line("entries.v", only_v.join(","));

  const pairs: string[] = [];
  for (const e of m.entries()) pairs.push(e[0] + ":" + e[1]);
  line("entries.pair", pairs.join(","));
}

// --- the loop body mutates the Map: the view is LIVE, not a snapshot ---
{
  const m = mkMap(2);
  const seen: number[] = [];
  for (const v of m.values()) {
    seen.push(v);
    if (seen.length === 1) m.set("appended", 99);
    if (seen.length > 8) break;
  }
  line("values.liveAppend", seen.join(","));
  line("values.liveAppend.size", m.size);
}
{
  const m = mkMap(6);
  const seen: string[] = [];
  for (const k of m.keys()) {
    seen.push(k);
    if (k === "k1") {
      m.delete("k3");
      m.delete("k4");
    }
  }
  line("keys.deleteAhead", seen.join(","));
}
{
  const m = mkMap(6);
  const seen: string[] = [];
  for (const k of m.keys()) {
    seen.push(k);
    m.delete(k);
  }
  line("keys.deleteCurrent", seen.join(","));
  line("keys.deleteCurrent.size", m.size);
}
{
  const m = mkMap(6);
  const seen: string[] = [];
  for (const k of m.keys()) {
    seen.push(k);
    if (k === "k3") m.delete("k0"); // below the cursor
  }
  line("keys.deleteBehind", seen.join(","));
}
{
  const m = mkMap(6);
  const seen: string[] = [];
  for (const [k, v] of m.entries()) {
    seen.push(k + "=" + v);
    if (k === "k1") m.delete("k2");
  }
  line("entries.deleteAhead", seen.join(","));
}

// --- single-ident heads bind a fresh, live [key, value] pair ---
{
  const m = mkMap(4);
  const out: string[] = [];
  for (const e of m) out.push(e[0] + "=" + e[1]);
  line("direct.pair", out.join(","));
  line("direct.isArray", Array.isArray([...m][0]));
  line("direct.len", [...m][0].length);

  const kept: unknown[] = [];
  for (const e of m) kept.push(e);
  line("direct.freshEachStep", kept[0] !== kept[1] && kept[1] !== kept[2]);

  const m2 = mkMap(2);
  const seen: string[] = [];
  for (const e of m2) {
    seen.push(String(e[0]));
    if (seen.length === 1) m2.set("late", 9);
    if (seen.length > 8) break;
  }
  line("direct.liveAppend", seen.join(","));

  const m3 = mkMap(4);
  const seen3: string[] = [];
  for (const e of m3) {
    seen3.push(String(e[0]));
    if (e[0] === "k0") m3.delete("k1");
  }
  line("direct.liveDelete", seen3.join(","));
}

// --- shapes the fast path must DECLINE: the value is destructured, not the
// --- entry, so the loop must still run the real `values()` iterator.
{
  const m = new Map<string, number[]>();
  m.set("a", [1, 2]);
  m.set("b", [3, 4]);
  const out: string[] = [];
  for (const [x, y] of m.values()) out.push(x + "/" + y);
  line("decline.destructuredValue", out.join(","));

  const s = new Map<string, string>([["a", "xy"]]);
  const out2: string[] = [];
  for (const [c0] of s.values()) out2.push(c0);
  line("decline.destructuredString", out2.join(","));
}

// --- Set views ---
{
  const s = new Set<number>([1, 2, 3, 4]);
  const a: number[] = [];
  for (const x of s.values()) a.push(x);
  line("set.values", a.join(","));
  const b: number[] = [];
  for (const x of s.keys()) b.push(x);
  line("set.keys", b.join(","));
  const c: string[] = [];
  for (const e of s.entries()) c.push(e[0] + "/" + e[1]);
  line("set.entries", c.join(","));

  const s2 = new Set<number>([1, 2, 3, 4, 5]);
  const d: number[] = [];
  for (const x of s2.values()) {
    d.push(x);
    if (x === 2) s2.delete(4);
  }
  line("set.values.liveDelete", d.join(","));
}

// --- the receiver reached through a class field, an object property, and a
// --- `Map | undefined` union parameter ---
{
  class Holder {
    m: Map<string, number> = new Map<string, number>();
  }
  const h = new Holder();
  h.m.set("a", 1);
  h.m.set("b", 2);
  const a: number[] = [];
  for (const v of h.m.values()) a.push(v);
  line("classField", a.join(","));

  const obj = { m: new Map<string, number>([["c", 3], ["d", 4]]) };
  const b: string[] = [];
  for (const k of obj.m.keys()) b.push(k);
  line("objectProp", b.join(","));

  function walk(m: Map<string, number> | undefined): string {
    if (!m) return "none";
    const out: string[] = [];
    for (const e of m) out.push(e[0] + "=" + e[1]);
    return out.join(",");
  }
  line("union", walk(new Map<string, number>([["u", 1], ["w", 2]])));
  line("union.undefined", walk(undefined));
}

// --- control flow out of a view loop ---
{
  const m = mkMap(6);
  const a: number[] = [];
  for (const v of m.values()) {
    if (v === 1) continue;
    if (v === 4) break;
    a.push(v);
  }
  line("breakContinue", a.join(","));

  const b: string[] = [];
  outer: for (const k of m.keys()) {
    for (const v of m.values()) {
      if (v === 2) continue outer;
      b.push(k + ">" + v);
      if (b.length > 20) break outer;
    }
  }
  line("nested", b.join(","));

  const seen: number[] = [];
  const first = (): number => {
    for (const v of m.values()) {
      seen.push(v);
      if (v === 2) return v;
    }
    return -1;
  };
  line("returnInLoop", first());
  line("returnInLoop.seen", seen.join(","));
}

// --- empty / one-entry / cleared collections ---
{
  const e = new Map<string, number>();
  const a: number[] = [];
  for (const v of e.values()) a.push(v);
  line("empty", "[" + a.join(",") + "]");

  const one = new Map<string, number>([["x", 1]]);
  const b: string[] = [];
  for (const k of one.keys()) b.push(k);
  line("one", b.join(","));

  const m = mkMap(4);
  m.clear();
  const c: number[] = [];
  for (const v of m.values()) c.push(v);
  line("cleared", "[" + c.join(",") + "]");
}

// --- non-string keys and mixed values through the views ---
{
  const m = new Map<unknown, unknown>();
  const o = { tag: "o" };
  m.set(1, "one");
  m.set(NaN, "nan");
  m.set(-0, "zero");
  m.set(o, "obj");
  m.set("s", null);
  const ks: string[] = [];
  for (const k of m.keys()) {
    ks.push(typeof k === "object" && k !== null ? "obj" : String(k));
  }
  line("mixed.keys", ks.join(","));
  const vs: string[] = [];
  for (const v of m.values()) vs.push(String(v));
  line("mixed.values", vs.join(","));
}

// --- iterating the same view expression twice, and a `let` head ---
{
  const m = mkMap(3);
  const a: number[] = [];
  for (const v of m.values()) a.push(v);
  for (const v of m.values()) a.push(v * 10);
  line("twice", a.join(","));

  const b: number[] = [];
  for (let v of m.values()) {
    v = v + 100;
    b.push(v);
  }
  line("letHead", b.join(","));
}

// --- `for await` over a Map: values and order.
//
// The microtask INTERLEAVING of a `for await` Map loop is a separate,
// pre-existing divergence and is deliberately not asserted here; the lowering
// it depends on is pinned instead by
// `perry-hir::lower::collection_view_tests::for_await_is_never_rewritten`,
// because a `.ts` assertion on it would be red for reasons this file is not
// about.
async function forAwaitOrder(): Promise<void> {
  const m = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
  const log: string[] = [];
  for await (const e of m) log.push("pair:" + e[0] + "=" + e[1]);
  for await (const [k, v] of m) log.push("kv:" + k + "=" + v);
  for await (const v of m.values()) log.push("v:" + v);

  // values that ARE promises: for-await must unwrap them
  const pm = new Map<string, Promise<number>>([
    ["p", Promise.resolve(10)],
    ["q", Promise.resolve(20)],
  ]);
  for await (const v of pm.values()) log.push("pv:" + v);

  line("forAwait", log.join("|"));
}

// --- scale: the shape `benchmarks/app-patterns/kernels/map_1m.ts` runs ---
{
  const N = 20000;
  const m = new Map<string, number>();
  for (let i = 0; i < N; i++) m.set("key_" + i, i * 2);
  let sum = 0;
  for (const v of m.values()) sum += v;
  let keyChars = 0;
  for (const k of m.keys()) keyChars += k.length;
  let pairSum = 0;
  for (const e of m) pairSum += e[1] as number;
  line("scale.values", sum);
  line("scale.keys", keyChars);
  line("scale.pairs", pairSum);
  line("scale.size", m.size);
}

// Runs last so its `line()` output lands after every synchronous case.
forAwaitOrder();
