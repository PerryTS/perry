// Old containers that span many pages keep receiving young children while
// minors run: a Map (the transposition-table shape), a Set, an Array, a class
// field array, a closure capture, and one large young array. Every value is
// read back after the churn. Negative controls: overwritten, deleted and
// cleared entries must read as what replaced them, and containers that stay
// on one page or die young take the other paths of the same code.

class Entry {
  key: string;
  depth: number;
  payload: { tag: string; n: number };
  constructor(key: string, depth: number) {
    this.key = key;
    this.depth = depth;
    this.payload = { tag: "p" + key, n: depth };
  }
}

class Holder {
  items: Entry[] = [];
  latest: Entry | undefined = undefined;
  add(e: Entry): void {
    this.items.push(e);
    this.latest = e;
  }
}

function churn(n: number): number {
  // Short-lived garbage: fills the nursery so minors happen between stores.
  let acc = 0;
  for (let i = 0; i < n; i++) {
    const tmp = { a: i, b: [i, i + 1, i + 2], c: "g" + i };
    acc += tmp.b[1] + tmp.c.length;
  }
  return acc;
}

function keyOf(i: number): string {
  return String(i * 7919) + ":" + String(i % 13) + ":w";
}

const table = new Map<string, Entry>();
const seen = new Set<Entry>();
const list: Entry[] = [];
const holder = new Holder();
const captured: Entry[] = [];
const readers: Array<() => string> = [];
const small: Array<Entry | number> = [0, 0, 0];
let garbage = 0;

const ROUNDS = 60;
const PER_ROUND = 500;
for (let round = 0; round < ROUNDS; round++) {
  for (let j = 0; j < PER_ROUND; j++) {
    const i = round * PER_ROUND + j;
    const e = new Entry(keyOf(i), i % 9);
    table.set(e.key, e);
    seen.add(e);
    list.push(e);
    holder.add(e);
    if (i % 50 === 0) {
      captured.push(e);
      const mine = e;
      readers.push(() => mine.key + "/" + mine.payload.tag);
    }
  }
  // An old one-page container gets a young child too.
  small[round % 3] = new Entry("small" + round, round);
  garbage += churn(4000);
  // A Map that is born, filled and dropped inside one round.
  const scratch = new Map<number, Entry>();
  for (let j = 0; j < 64; j++) scratch.set(j, new Entry("s" + j, j));
  garbage += scratch.size;
}

// One large array born late: young while the old containers point into it.
const large: Entry[] = new Array<Entry>(40000);
for (let i = 0; i < large.length; i++) large[i] = list[i % list.length]!;
const late = new Entry("late", 1);
table.set("late", late);
large[39999] = late;
garbage += churn(20000);

const total = ROUNDS * PER_ROUND;
let ok = 0;
let bad = 0;
for (let i = 0; i < total; i++) {
  const k = keyOf(i);
  const e = table.get(k);
  if (e !== undefined && e.key === k && e.depth === i % 9 && e.payload.tag === "p" + k && e.payload.n === i % 9 &&
      list[i] === e && holder.items[i] === e && seen.has(e)) ok++;
  else bad++;
}
console.log("entries", table.size, seen.size, list.length, holder.items.length);
console.log("intact", ok, "broken", bad);
console.log("latest", holder.latest?.key, holder.latest?.payload.tag);
console.log("captured", captured.length, readers.length, readers[0]!(), readers[readers.length - 1]!());
let capturedOk = 0;
for (let i = 0; i < captured.length; i++) if (readers[i]!() === captured[i]!.key + "/p" + captured[i]!.key) capturedOk++;
console.log("capturedOk", capturedOk);
console.log("small", small.map((v) => (typeof v === "number" ? "n" : v.key)).join(","));
console.log("large", large.length, large[0]!.key, large[39999]!.key, table.get("late") === large[39999]);

// Negative controls.
for (let i = 0; i < total; i += 3) table.delete(keyOf(i));
for (let i = 1; i < total; i += 3) table.set(keyOf(i), new Entry("replaced" + i, -1));
for (let i = 0; i < total; i += 2) list[i] = undefined as unknown as Entry;
garbage += churn(20000);
let deleted = 0;
let replaced = 0;
let kept = 0;
let holes = 0;
for (let i = 0; i < total; i++) {
  const e = table.get(keyOf(i));
  if (e === undefined) deleted++;
  else if (e.key === "replaced" + i && e.depth === -1 && e.payload.tag === "preplaced" + i) replaced++;
  else if (e.key === keyOf(i)) kept++;
  if (list[i] === undefined) holes++;
}
console.log("deleted", deleted, "replaced", replaced, "kept", kept, "holes", holes, "size", table.size);
seen.clear();
table.clear();
garbage += churn(20000);
console.log("cleared", table.size, seen.size, table.get(keyOf(1)), seen.has(late));
for (let i = 0; i < 2000; i++) table.set(keyOf(i), new Entry(keyOf(i), 3));
garbage += churn(20000);
let refilled = 0;
for (let i = 0; i < 2000; i++) if (table.get(keyOf(i))!.payload.tag === "p" + keyOf(i)) refilled++;
console.log("refilled", refilled, table.size);
console.log("garbage", garbage);
