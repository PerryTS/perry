// lastIndexOf with and without fromIndex, in sync and async bodies: a negative
// fromIndex counts from the end, an explicit undefined searches from index 0, and
// NaN is never found (strict equality).
const a = [1, 2, 3, 2, 1];
console.log(a.lastIndexOf(2), a.lastIndexOf(2, 2), a.lastIndexOf(1, -2), a.lastIndexOf(9), a.lastIndexOf(1, undefined));
console.log(a.lastIndexOf(1, -6), a.lastIndexOf(1, 99), a.lastIndexOf(2, 1.7));
const s = ["x", "y", "x"];
console.log(s.lastIndexOf("x"), s.lastIndexOf("x", 1), [NaN, 1].lastIndexOf(NaN));
const o = { k: 1 };
const objs: object[] = [o, {}, o];
console.log(objs.lastIndexOf(o), objs.lastIndexOf(o, 1), objs.lastIndexOf({}));
async function run() {
  const b = [5, 6, 5];
  console.log("async", b.lastIndexOf(5), b.lastIndexOf(5, 1), b.lastIndexOf(6, -3));
}
run();
