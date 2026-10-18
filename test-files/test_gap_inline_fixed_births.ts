// Fixed closure/layout births must preserve identity, captures and late holes
// across nursery refills and moving collections. The GC stress run supplies gc.
class RecordCell {
  value: any;
  constructor(value: any) { this.value = value; }
}
function small(seed: number) {
  const object = { value: seed };
  let count = seed;
  return { read: () => object.value + count, step: () => ++count };
}
function wide(seed: number) {
  const a = { n: seed }, b = { n: seed + 1 }, c = { n: seed + 2 };
  const d = { n: seed + 3 }, e = { n: seed + 4 };
  return () => a.n + b.n + c.n + d.n + e.n;
}
function receiver(seed: number) {
  return { seed, read() { return this.seed; } };
}
function empty() { return () => 7; }
const kept: any[] = [];
let sum = 0;
for (let i = 0; i < 40000; i++) {
  const pair = small(i);
  pair.step();
  sum += pair.read();
  const fn = wide(i);
  const cell: any = new RecordCell({ read: fn });
  if (i % 4000 === 0) kept.push({ pair, cell, method: receiver(i) });
  if (i % 997 === 0 && typeof (globalThis as any).gc === 'function') (globalThis as any).gc();
}
console.log(sum);
for (const item of kept) {
  console.log(item.pair.read(), item.cell.value.read(), item.method.read());
  item.pair.step();
  console.log(item.pair.read(), Object.keys(item.cell).join(','));
}
console.log(empty() === empty(), empty()());
const first = small(1), second = small(1);
console.log(first.read === second.read);
(first.read as any).tag = 42;
console.log((first.read as any).tag, (second.read as any).tag);
const map = new WeakMap(); map.set(first.read, 'first');
console.log(map.get(first.read), map.has(second.read));
