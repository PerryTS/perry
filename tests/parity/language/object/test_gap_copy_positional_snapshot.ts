// Copy the internal key snapshot, with source attributes deciding admission.
const source: any = { short: 1, longerName: 2, "2": 3, "1": 4 };
Object.defineProperty(source, "hidden", { value: 5, enumerable: false });
const sym = Symbol("shown");
source[sym] = 6;
const a: any = Object.assign({}, source);
const b: any = { ...source, short: 7 };
console.log(Object.keys(a).join(","), JSON.stringify(a), a[sym]);
console.log(Object.keys(b).join(","), JSON.stringify(b), b[sym]);
const plain: any = { short: 1, longerName: 2, thirdName: 3 };
const same = Object.assign(plain, plain);
console.log(same === plain, JSON.stringify(same));
let calls = 0;
const getter: any = { get short() { calls++; delete this.tail; return 8; }, tail: 9 };
console.log(JSON.stringify({ ...getter }), calls);
const array: any = [1, , 3];
array.extra = 4;
console.log(JSON.stringify(Object.assign({}, array)));
const keys: any = {};
for (let i = 0; i < 40; i++) keys["field_" + i] = i;
const copy: any = { ...keys, field_0: 99 };
console.log(Object.keys(copy).length, copy.field_0, copy.field_39);
// Fresh heap values and long keys cross collection thresholds while each
// spread grows spill storage. The store owns the roots for its own operands.
let total = 0;
for (let r = 0; r < 2000; r++) {
  const fresh: any = {};
  for (let i = 0; i < 40; i++) fresh["long_field_" + i] = { n: r * 40 + i };
  const spread: any = { ...fresh };
  total += spread.long_field_0.n + spread.long_field_39.n;
}
console.log(total);
