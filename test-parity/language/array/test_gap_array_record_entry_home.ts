function collect() {
  const gc = (globalThis as any).gc;
  if (gc) gc();
  const junk: any[] = [];
  for (let i = 0; i < 80; i++) junk.push({ i, nested: { i } });
}
let evaluations = 0, gets = 0;
const items: any = [{ n: 1 }, { n: 2 }, { n: 3 }];
function source(): any { evaluations++; collect(); return items; }
let sum = 0;
for (const value of source()) { collect(); sum += value.n; }
console.log("array", sum, evaluations);
Object.defineProperty(items, Symbol.iterator, {
  configurable: true,
  get() {
    gets++; collect();
    let i = 0;
    return function() {
      const iterator: any = {
        get next() { collect(); return function() { collect(); return i < 3 ? { value: items[i++], done: false } : { done: true }; }; }
      };
      return iterator;
    };
  }
});
sum = 0;
for (const value of source()) { collect(); sum += value.n; }
console.log("protocol", sum, evaluations, gets);
