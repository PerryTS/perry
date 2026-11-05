// Charter step 5 (DESIGN §5.2): a lineage slot that sees a non-Number in 1 of
// 1000 key-adds converges on one shape. The key-add of `v` is learned from a
// Number, then a string arrives; after it, Number key-adds must be born into
// the `Any` shape (the F64 sibling is deprecated), so the objects all share
// one shape and a later store site stays monomorphic. Run with
// PERRY_STORE_CENSUS=1 (compile + run): the key-add and store misses stay
// near the number of generalizations, not near the number of objects.
declare function gc(): void;
function fresh(): any {
  return {};
}
function addV(o: any, v: any): void {
  o.v = v;
}
function setV(o: any, v: any): void {
  o.v = v;
}
const objs: any[] = [];
for (let i = 0; i < 20000; i++) {
  const o = fresh();
  addV(o, i % 1000 === 7 ? "s" + i : i + 0.5);
  objs.push(o);
}
for (let r = 0; r < 5; r++) {
  for (let i = 0; i < objs.length; i++) setV(objs[i], i % 1000 === 7 ? "t" + i : i + r);
}
if (typeof gc === "function") gc();
let strings = 0;
let sum = 0;
for (const o of objs) {
  if (typeof o.v === "string") strings++;
  else sum += o.v;
}
console.log(strings, sum);
