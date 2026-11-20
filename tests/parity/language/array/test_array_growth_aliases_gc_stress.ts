// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11830 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// test_array_growth_aliases.ts under a collection every few KB of allocation,
// with from-space protected so a reference left on a moved head faults.
// An array that grows past its capacity must stay ONE array for every
// reference to it: a second binding, a closure capture, an object field, a
// nested array slot, and identity-keyed Map/Set entries. Growth either
// lengthens the array where it stands (when it was the last nursery birth) or
// moves it and leaves a forwarding stub, so each case is run with nothing
// allocated between the pushes (in place) and with allocation pressure
// between them (copy + stub, and collections while aliases hold the head).

let sink: unknown[] = [];
function pressure(k: number): void {
  // Short-lived garbage plus a few survivors, enough to run minor
  // collections while the aliases below are live.
  const tmp: { k: number; s: string }[] = [];
  for (let j = 0; j < 40; j++) tmp.push({ k: k + j, s: "p" + (k + j) });
  if (k % 7 === 0) sink.push(tmp[k % 40]);
  if (sink.length > 64) sink = [];
}

function digest(xs: unknown[]): string {
  let h = 0;
  for (let i = 0; i < xs.length; i++) {
    const v = xs[i];
    const n = typeof v === "number" ? v : typeof v === "string" ? v.length : v === undefined ? -1 : 7;
    h = (h * 31 + n + i) % 1000003;
  }
  return xs.length + ":" + h;
}

function aliases(withPressure: boolean, n: number): string {
  const a: number[] = [];
  const b = a;
  const holder = { xs: a };
  const nest: number[][] = [a];
  const ids = new Map<number[], string>();
  ids.set(a, "a");
  const seen = new Set<number[]>([a]);
  const push = (v: number): number => a.push(v);
  for (let i = 0; i < n; i++) {
    if (i % 3 === 0) push(i);
    else if (i % 3 === 1) holder.xs.push(i);
    else b.push(i);
    if (withPressure) pressure(i);
  }
  return [
    a.length, b.length, holder.xs.length, nest[0].length,
    a === b, holder.xs === a, nest[0] === b,
    ids.get(b), ids.get(holder.xs), seen.has(nest[0]),
    b[0], b[n - 1], holder.xs[n >> 1], nest[0][n - 2],
    digest(b), digest(holder.xs),
  ].join(" ");
}

for (const n of [3, 16, 17, 33, 40, 65, 200, 1100]) {
  console.log("alias", n, aliases(false, n));
  console.log("alias+gc", n, aliases(true, n));
}

// Pointer elements: objects and strings pushed with collections in between,
// read back through every alias.
function objects(withPressure: boolean): string {
  const xs: { a: number; tag: string }[] = [];
  const view = { list: xs };
  const capture = () => xs;
  for (let k = 0; k < 90; k++) {
    xs.push({ a: k, tag: "t" + k });
    if (withPressure) pressure(k);
  }
  let sum = 0;
  for (const o of capture()) sum += o.a + o.tag.length;
  return [xs.length, view.list.length, capture().length, sum, view.list[89].tag, capture()[45].a].join(" ");
}
console.log("objects", objects(false));
console.log("objects+gc", objects(true));

// Two arrays filled in turn: each push makes the other one the last birth, so
// growth alternates between in place and a moved copy.
function interleaved(): string {
  const p: number[] = [];
  const q: string[] = [];
  const pa = p;
  const qa = q;
  for (let i = 0; i < 70; i++) {
    p.push(i);
    q.push("q" + i);
    if (i % 2 === 0) p.push(-i);
  }
  return [p.length, pa.length, q.length, qa.length, pa[104], qa[69], digest(pa), digest(qa)].join(" ");
}
console.log("interleaved", interleaved());

// An array kept alive across many collections (promoted / tenured), then
// grown, still seen by its alias and by an object that also survived.
function survivor(): string {
  const old: number[] = [1, 2, 3];
  const keep = { old };
  for (let r = 0; r < 300; r++) pressure(r);
  for (let i = 0; i < 50; i++) old.push(i * 2);
  for (let r = 0; r < 100; r++) pressure(r);
  for (let i = 0; i < 50; i++) keep.old.push(-i);
  return [old.length, keep.old.length, keep.old === old, old[52], keep.old[102], digest(keep.old)].join(" ");
}
console.log("survivor", survivor());

// Holes, an explicit length, and a store past the end before more pushes.
function sparse(): string {
  const s: (number | undefined)[] = [];
  const t = s;
  for (let i = 0; i < 10; i++) s.push(i);
  s.length = 20;
  s.push(99);
  s[30] = 7;
  for (let i = 0; i < 20; i++) t.push(i);
  return [s.length, t.length, 15 in s, s[20], s[30], 31 in t, t[50], JSON.stringify(t.slice(18, 24))].join(" ");
}
console.log("sparse", sparse());

// Mixed element kinds across growth, then pop/slice/join on an alias.
function mixed(): string {
  const m: unknown[] = [];
  const alias = m;
  for (let i = 0; i < 20; i++) m.push(i + 0.5);
  for (let i = 0; i < 20; i++) m.push("s" + i);
  for (let i = 0; i < 20; i++) m.push({ i });
  m.push(undefined, null, true);
  const popped = alias.pop();
  return [m.length, alias.length, popped, typeof alias[45], (alias[50] as { i: number }).i, alias.slice(17, 22).join("|")].join(" ");
}
console.log("mixed", mixed());

// Integrity: a frozen or non-extensible full array still refuses the push.
for (const lock of ["freeze", "preventExtensions", "seal"]) {
  const z: number[] = [];
  for (let i = 0; i < 16; i++) z.push(i);
  if (lock === "freeze") Object.freeze(z);
  else if (lock === "seal") Object.seal(z);
  else Object.preventExtensions(z);
  try {
    z.push(1);
    console.log(lock, "no throw", z.length);
  } catch (e) {
    console.log(lock, (e as Error).constructor.name, z.length);
  }
}

// A subclass instance and an array with an own expando grow too.
class Bag extends Array<number> {}
const bag = new Bag();
const bagAlias = bag;
for (let i = 0; i < 40; i++) bag.push(i);
console.log("subclass", bag.length, bagAlias.length, bagAlias[39], bag instanceof Bag);
const tagged: number[] & { label?: string } = [];
tagged.label = "kept";
const taggedAlias = tagged;
for (let i = 0; i < 40; i++) tagged.push(i);
console.log("expando", tagged.length, taggedAlias.length, taggedAlias.label, taggedAlias[39]);
