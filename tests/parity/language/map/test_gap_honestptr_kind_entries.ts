// Kind-typed entries selected by construction: a Map/Set the compiler
// allocated calls the typed size/get entries with no receiver check. Every
// other receiver (a subclass instance, a declared-but-plain object, a
// reassigned binding) keeps the checked generic entry. Allocation between
// uses moves the collections under the moving collector.
function churn(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: [i, i + 1], c: "s" + i };
    total += o.b.length;
  }
  return total;
}

function constructed(): void {
  const m = new Map<string, number>();
  const s = new Set<number>();
  for (let i = 0; i < 2000; i++) {
    m.set("k" + i, i);
    s.add(i % 97);
    if (i % 500 === 0) churn(3000);
  }
  console.log("map size", m.size, "set size", s.size);
  let sum = 0;
  for (let i = 0; i < 2000; i += 7) {
    sum += m.get("k" + i) ?? 0;
  }
  console.log("get sum", sum, "absent", m.get("nope"));
  const fromArray = new Map([[1, "one"], [2, "two"]]);
  console.log("from array", fromArray.size, fromArray.get(2));
  const setFrom = new Set([1, 2, 2, 3]);
  console.log("set from", setFrom.size);
}

class CountingMap extends Map<string, number> {
  get size(): number {
    return super.size + 100;
  }
}

function notConstructed(): void {
  const sub = new CountingMap();
  sub.set("a", 1);
  console.log("subclass size", sub.size);
  let m = new Map<number, number>();
  m.set(1, 1);
  m = new Map<number, number>([[5, 6]]);
  console.log("reassigned", m.size, m.get(5));
  const fake = { size: 42 } as unknown as Map<number, number>;
  console.log("declared only", fake.size);
}

constructed();
notConstructed();
