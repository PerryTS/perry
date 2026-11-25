// indexOf, lastIndexOf and includes search a typed array as they search an array, with
// and without fromIndex, in a plain and an async body: NaN is never found by index and
// includes finds it, -0 is found as 0. On `--target web` the search bridges once
// answered only arrays, so a typed array gave -1 / false (#15396).

const i = new Int32Array([1, 2, 3, 2, 1]);
const f = new Float64Array([NaN, 0.5, -0]);
const u = new Uint8Array([7, 8, 7]);
console.log(i.indexOf(2), i.indexOf(2, 2), i.lastIndexOf(2), i.lastIndexOf(2, 2), i.lastIndexOf(1, -2), i.lastIndexOf(9));
console.log(i.includes(3), i.includes(3, 3), i.lastIndexOf(1, -6), i.indexOf(1, -1));
console.log(f.indexOf(NaN), f.includes(NaN), f.lastIndexOf(0), f.includes(0.5, -1));
console.log(u.lastIndexOf(7), u.indexOf(8), u.includes(9), new Uint16Array([4, 4]).lastIndexOf(4, 0));

async function run(): Promise<void> {
  const a = new Int16Array([5, 6, 5]);
  const g = new Float32Array([1.5, 2.5]);
  console.log("async", a.lastIndexOf(5), a.lastIndexOf(5, 1), a.indexOf(6), a.includes(5, 3), g.indexOf(2.5), g.includes(1.5));
}
run();
