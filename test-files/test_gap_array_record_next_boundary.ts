function collect() {
  const gc = (globalThis as any).gc;
  if (gc) gc();
  const junk: any[] = [];
  for (let i = 0; i < 80; i++) junk.push({ i, child: { i } });
}
let reads = 0, closes = 0;
const a: any = [1, 2, 3];
const proto = Object.create(Array.prototype);
Object.defineProperty(proto, "2", { get() { collect(); reads++; return 42; }, configurable: true });
Object.setPrototypeOf(a, proto);
Object.defineProperty(a, "1", { get() { collect(); reads++; delete a[2]; return 5; }, configurable: true });
let total = 0, count = 0;
for (const x of a) { collect(); total += x; count++; }
console.log("get", total, count, reads);
const iteratorProto: any = Object.getPrototypeOf([][Symbol.iterator]());
const oldReturn = iteratorProto.return;
iteratorProto.return = function() { collect(); closes++; return { done: true }; };
try {
  const b: any = [1, 2];
  Object.defineProperty(b, "1", { get() { collect(); throw new Error("get"); } });
  try { for (const x of b) { collect(); } } catch (e) { console.log("step", (e as Error).message, closes); }
  try { for (const x of [1, 2]) { collect(); throw new Error("body"); } } catch (e) { console.log("body", (e as Error).message, closes); }
  const c: any[] = [{ n: 1 }, { n: 2 }];
  let sum = 0;
  for (const x of c) {
    collect();
    sum += x.n;
    if (sum === 1) c.push({ n: 3 });
  }
  console.log("grow", sum, closes);
} finally {
  if (oldReturn === undefined) delete iteratorProto.return;
  else iteratorProto.return = oldReturn;
}
