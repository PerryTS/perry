// Map get-then-set fusion: the key (or record) binding changes between the get and
// the set with no call in between. Only the key-bit comparison tells the entries apart.
const dump = (label: string, m: Map<any, any>): void => {
  const parts: string[] = [];
  m.forEach((v: any, k: any) => {
    parts.push((typeof k === "object" ? "obj" : Object.is(k, -0) ? "-0" : String(k)) + "=" + (typeof v === "string" ? v.slice(0, 12) + "#" + v.length : typeof v === "object" ? "obj" : String(v)));
  });
  console.log(label + " size=" + m.size + " [" + parts.join(",") + "]");
};
const churn = (n: number): number => {
  let s = "";
  for (let i = 0; i < n; i++) s += "x" + i;
  return s.length;
};

// 17. key reassigned between get and set with no call in between: only the
// key-bit comparison tells the two entries apart
{
  const m = new Map<any, any>([["a", 1], ["b", 10]]);
  let k: any = "a";
  const cur = m.get(k);
  k = "b";
  m.set(k, cur + 1);
  dump("key-swap", m);
  let j: any = "a";
  for (let i = 0; i < 4; i++) {
    const c = m.get(j);
    j = i % 2 === 0 ? "b" : "a";
    m.set(j, c + 5);
  }
  dump("key-swap-loop", m);
}
// 18. typed form (generic key arm): the record binding changes between get and
// set with no call in between; the loop alternates the entry written
{
  type R = { cat: string; value: number };
  const ra: R[] = [];
  for (const s of ["a", "b", "c"]) ra.push({ cat: s + "", value: 1 });
  const m: Map<string, number> = new Map();
  m.set("a", 1);
  m.set("b", 10);
  m.set("c", 100);
  let r: R = ra[0];
  const cur: number | undefined = m.get(r.cat);
  r = ra[1];
  m.set(r.cat, (cur === undefined ? 0 : cur) + r.value);
  dump("typed-swap", m);
  for (let i = 0; i < 6; i++) {
    let q: R = ra[i % 3];
    const c: number | undefined = m.get(q.cat);
    q = ra[(i + 1) % 3];
    m.set(q.cat, (c === undefined ? 0 : c) + q.value);
  }
  dump("typed-swap-loop", m);
}
