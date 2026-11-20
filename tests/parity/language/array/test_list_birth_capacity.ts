// The birth capacity of a push-grown list (wpfacts `list_caps`): a `[]`
// local a repeated push reaches is born with more slots. It must stay an
// ordinary empty Array whatever reaches it: length, holes past the end,
// growth past the reserve, a length set, an escape, a list never pushed.

// 1. The chess shape: born `[]`, pushed in nested loops and by a callee
//    that pushes into its parameter, returned.
function addPair(out: number[], a: number, b: number): void {
  out.push(a * 100 + b);
  if (b % 3 === 0) out.push(-(a * 100 + b));
}
function generate(n: number): number[] {
  const moves: number[] = [];
  for (let from = 0; from < n; from++) {
    if ((from & 1) !== 0) {
      addPair(moves, from, from + 1);
      continue;
    }
    let to = from;
    while (to < n && to < from + 3) {
      moves.push(from * 100 + to);
      to++;
    }
  }
  if (n > 2) moves.push(-1);
  return moves;
}
for (const n of [0, 1, 2, 5, 13, 14, 15, 27, 28, 60, 300]) {
  const m = generate(n);
  let sum = 0;
  for (const v of m) sum += v;
  console.log("generate", n, m.length, sum, m[m.length - 1], m[m.length], m.length in m);
}

// 2. Past the reserved capacity, by one and by a lot.
function count(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i);
  return out;
}
for (const n of [39, 40, 41, 63, 64, 65, 80, 81, 1000, 5000]) {
  const a = count(n);
  console.log("count", n, a.length, a[0], a[n - 1], a[n], a.indexOf(n - 1), a.includes(n));
}

// 3. A static push bound: only straight-line pushes (6 here, 50 below).
function six(x: number): number[] {
  const out: number[] = [];
  out.push(x);
  out.push(x + 1);
  if (x > 2) out.push(x + 2);
  out.push(x + 3, x + 4);
  out.push(x + 5);
  return out;
}
console.log("six", JSON.stringify(six(1)), JSON.stringify(six(7)), six(1).length, six(7).length);
function fifty(x: number): number[] {
  const out: number[] = [];
  out.push(x, 1); out.push(x, 2); out.push(x, 3); out.push(x, 4); out.push(x, 5);
  out.push(x, 6); out.push(x, 7); out.push(x, 8); out.push(x, 9); out.push(x, 10);
  out.push(x, 11); out.push(x, 12); out.push(x, 13); out.push(x, 14); out.push(x, 15);
  out.push(x, 16); out.push(x, 17); out.push(x, 18); out.push(x, 19); out.push(x, 20);
  out.push(x, 21); out.push(x, 22); out.push(x, 23); out.push(x, 24); out.push(x, 25);
  return out;
}
{
  const f = fifty(9);
  let s = 0;
  for (let i = 0; i < f.length; i++) s += f[i] * (i + 1);
  console.log("fifty", f.length, s, f[49], f[50]);
}

// 4. Never pushed, and a loop that never runs its push.
function never(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    if (i > n) out.push(i);
  }
  return out;
}
{
  const e = never(8);
  const plain: number[] = [];
  console.log("never", e.length, e[0], 0 in e, JSON.stringify(e), plain.length, JSON.stringify(plain));
}

// 5. Holes and a length set: a store past the end, `length = k` both ways.
function holes(n: number): (number | undefined)[] {
  const out: (number | undefined)[] = [];
  for (let i = 0; i < n; i++) out.push(i);
  out[n + 3] = 99;
  return out;
}
{
  const h = holes(5);
  console.log("holes", h.length, h[5], 5 in h, 8 in h, h[8], JSON.stringify(h));
  const keys: string[] = [];
  for (const k in h) keys.push(k);
  console.log("holes keys", keys.join(","));
  let seen = 0;
  h.forEach(() => seen++);
  console.log("holes forEach", seen);
}
function lengths(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i + 1);
  out.length = 3;
  out.push(7);
  out.length = 30;
  out.push(8);
  return out;
}
{
  const l = lengths(10);
  console.log("lengths", l.length, l[2], l[3], l[4], 4 in l, l[30], JSON.stringify(l.slice(0, 6)));
  const z: number[] = [];
  for (let i = 0; i < 20; i++) z.push(i);
  z.length = 0;
  console.log("lengths zero", z.length, z[0], 0 in z);
  for (let i = 0; i < 50; i++) z.push(i * 2);
  console.log("lengths refill", z.length, z[49]);
}

// 6. A stack: pushed and popped in one loop.
function stack(n: number): number {
  const st: number[] = [];
  let acc = 0;
  for (let i = 0; i < n; i++) {
    st.push(i);
    if (i % 3 === 2) acc += (st.pop() ?? 0) + (st.pop() ?? 0);
  }
  while (st.length > 0) acc = acc * 3 + (st.pop() ?? 0);
  return acc % 1000003;
}
console.log("stack", stack(0), stack(7), stack(100), stack(1000));

// 7. An escape: the list is stored in an object and a module array, and
//    written elsewhere (pushes, an index store, a sort) through those.
const registry: number[][] = [];
class Holder {
  items: number[] = [];
  adopt(list: number[]): void { this.items = list; }
  more(n: number): void { for (let i = 0; i < n; i++) this.items.push(1000 + i); }
}
function escaping(n: number, holder: Holder): number[] {
  const list: number[] = [];
  holder.adopt(list);
  registry.push(list);
  for (let i = 0; i < n; i++) list.push(n - i);
  return list;
}
{
  const holder = new Holder();
  const l = escaping(12, holder);
  holder.more(45);
  registry[0][2] = -5;
  registry[0].sort((a, b) => a - b);
  console.log("escape", l.length, l === holder.items, l === registry[0], l[0], l[1], l[56], l[57]);
  const frozen = escaping(3, new Holder());
  Object.freeze(frozen);
  let threw = false;
  try { frozen.push(1); } catch { threw = true; }
  console.log("escape frozen", frozen.length, threw, Object.isFrozen(frozen));
}

// 8. Aliases, closures and a spread push.
function aliased(n: number): number {
  const a: number[] = [];
  const b = a;
  const add = (v: number) => { b.push(v); };
  for (let i = 0; i < n; i++) add(i);
  a.push(...b.slice(0, 3));
  for (let i = 0; i < n; i++) b.push(...[i, i]);
  return a.length * 1000 + (a[a.length - 1] ?? -1);
}
console.log("aliased", aliased(0), aliased(2), aliased(30), aliased(100));

// 9. Lists of pointers and strings; per-iteration lists; nested lists.
function words(n: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < n; i++) out.push("w" + i);
  return out;
}
function rows(n: number): number[][] {
  const out: number[][] = [];
  for (let r = 0; r < n; r++) {
    const row: number[] = [];
    for (let c = 0; c <= r; c++) row.push(r * c);
    out.push(row);
  }
  return out;
}
{
  const w = words(70);
  console.log("words", w.length, w[0], w[69], w[70], w.join("").length);
  const r = rows(45);
  console.log("rows", r.length, r[44].length, r[44][44], r[0].length, r.map((x) => x.length).reduce((a, b) => a + b, 0));
  let total = 0;
  for (let i = 0; i < 2000; i++) {
    const t: number[] = [];
    for (let k = 0; k < (i % 50); k++) t.push(k + i);
    total = (total + t.length * 7 + (t[t.length - 1] ?? 0)) % 1000003;
  }
  console.log("churn", total);
}

// 10. Module-level lists.
const top: number[] = [];
for (let i = 0; i < 100; i++) top.push(i * i);
const once: number[] = [];
once.push(1);
console.log("top", top.length, top[99], top[100], once.length, once[0], once[1]);
