// Map get-then-set: one lookup when nothing can intervene, ordinary semantics
// otherwise. Typed Map<string, number> with record-field keys (the generic-key
// arm that fuses). Every case must match Node, including entry order.
type R = { cat: string; value: number };
const rec = (cat: string, value: number): R => ({ cat, value });
const dump = (label: string, m: Map<string, any>): void => {
  const parts: string[] = [];
  m.forEach((v: any, k: string) => {
    parts.push(k + "=" + (typeof v === "string" ? v.slice(0, 12) + "#" + v.length : String(v)));
  });
  console.log(label + " size=" + m.size + " [" + parts.join(",") + "]");
};
const churn = (n: number): number => {
  let s = "";
  for (let i = 0; i < n; i++) s += "x" + i;
  return s.length;
};
const mk = (...pairs: [string, number][]): Map<string, number> => {
  const m: Map<string, number> = new Map();
  for (const [k, v] of pairs) m.set(k, v);
  return m;
};

// 1. counting idiom, expression form
{
  const m: Map<string, number> = new Map();
  const rs: R[] = ["a", "b", "a", "c", "a", "b"].map((c) => rec(c + "", 1));
  for (let i = 0; i < rs.length; i++) {
    const r = rs[i];
    m.set(r.cat, ((m.get(r.cat) as number | undefined) ?? 0) + r.value);
  }
  dump("count", m);
}
// 2. statement form, content-equal keys at different addresses
{
  const m: Map<string, number> = new Map();
  for (let i = 0; i < 12; i++) {
    const r = rec("k" + (i % 4), i);
    const cur: number | undefined = m.get(r.cat);
    m.set(r.cat, (cur === undefined ? 0 : cur) + r.value);
  }
  dump("stmt", m);
}
// 3. the value computation deletes the key
{
  const m = mk(["a", 3], ["b", 4]);
  const r = rec("a", 0);
  const evil: any = { valueOf() { m.delete("a"); return 5; } };
  m.set(r.cat, (m.get(r.cat) as number) + evil);
  dump("delete-in-value", m);
}
{
  const m = mk(["a", 3], ["b", 4]);
  const r = rec("a", 0);
  const cur: number | undefined = m.get(r.cat);
  m.delete("a");
  m.set(r.cat, (cur as number) + 1);
  dump("delete-between", m);
}
// 4. the value computation clears the Map
{
  const m = mk(["a", 3], ["b", 4]);
  const r = rec("a", 0);
  const evil: any = { valueOf() { m.clear(); return 5; } };
  m.set(r.cat, (m.get(r.cat) as number) + evil);
  dump("clear-in-value", m);
}
// 5. the value computation sets the same key, then the outer set wins
{
  const m = mk(["a", 3], ["b", 4]);
  const r = rec("a", 0);
  const evil: any = { valueOf() { m.set("a", 100); return 5; } };
  m.set(r.cat, (m.get(r.cat) as number) + evil);
  dump("set-in-value", m);
}
// 6. the value computation grows the Map (moves the entry storage)
{
  const m = mk(["a", 3], ["b", 4]);
  const r = rec("a", 0);
  const evil: any = { valueOf() { for (let i = 0; i < 10000; i++) m.set("g" + i, i); return 5; } };
  m.set(r.cat, (m.get(r.cat) as number) + evil);
  console.log("grow-in-value", m.get("a"), m.size, m.keys().next().value);
}
// 7. growth, deletion and compaction in the value computation
{
  const m: Map<string, number> = new Map();
  for (let i = 0; i < 100; i++) m.set("k" + i, i);
  const r = rec("k0", 0);
  const evil: any = {
    valueOf() {
      for (let i = 1; i < 100; i++) m.delete("k" + i);
      for (let i = 0; i < 300; i++) m.set("n" + i, i);
      return 1;
    },
  };
  m.set(r.cat, (m.get(r.cat) as number) + evil);
  console.log("compact-in-value", m.get("k0"), m.size, m.keys().next().value);
}
// 8. allocation between get and set (moving collections)
{
  const m = mk(["a", 1]);
  const r = rec("a", 0);
  for (let i = 0; i < 20; i++) m.set(r.cat, (m.get(r.cat) as number) + churn(2000));
  dump("alloc", m);
}
// 9. absent key whose value computation inserts it
{
  const m: Map<string, number> = new Map();
  const r = rec("z", 0);
  const evil: any = { valueOf() { m.set("z", 1); return 5; } };
  m.set(r.cat, ((m.get(r.cat) as number | undefined) ?? 0) + evil);
  dump("absent-inserted", m);
}
// 10. absent key, plain insertion order
{
  const m = mk(["a", 1], ["b", 2]);
  for (const c of ["x", "a", "y", "b", "x"]) {
    const r = rec(c, 1);
    m.set(r.cat, ((m.get(r.cat) as number | undefined) ?? 0) + r.value);
  }
  dump("insert-order", m);
}
// 11. pointer-valued values keep their barrier
{
  const m: Map<string, any> = new Map();
  const r = rec("s", 0);
  for (let i = 0; i < 6; i++) m.set(r.cat, ((m.get(r.cat) as any) ?? "") + "ab" + churn(300));
  dump("str", m);
  const m2: Map<string, any> = new Map();
  m2.set("o", { n: 1 });
  const r2 = rec("o", 0);
  for (let i = 0; i < 5; i++) {
    const cur: any = m2.get(r2.cat);
    m2.set(r2.cat, { n: cur.n + 1, pad: churn(500) });
  }
  console.log("ptr", m2.get("o").n);
}
// 12. receiver reassigned between get and set
{
  let m: Map<string, number> = mk(["a", 1]);
  const other: Map<string, number> = mk(["a", 50]);
  const r = rec("a", 0);
  const cur: number | undefined = m.get(r.cat);
  m = other;
  m.set(r.cat, (cur as number) + 1);
  dump("reassigned", m);
}
