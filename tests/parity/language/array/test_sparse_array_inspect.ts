// util.inspect / console.log / util.format of a sparse Array: a short store
// under a long `length` (`a[N] = x`, `a.length = N`, `new Array(N)`). The far
// elements are own properties of the Array, everything between is a run of
// holes, printed as ONE `<k empty items>` entry. No word behind the store is
// ever printed. Negative controls: dense Arrays with and without holes.
import * as util from "node:util";

const N = 3000000;
const cases: [string, () => any[]][] = [
  ["far store", () => { const a: any[] = []; a[N] = 1; return a; }],
  ["dense head, far tail", () => { const a: any[] = ["a0", 11]; a[N] = "far"; return a; }],
  ["length set", () => { const a: any[] = [1, 2]; a.length = N; return a; }],
  ["length set, far store", () => { const a: any[] = [5]; a.length = N; a[N - 1] = "z"; return a; }],
  ["new Array(N)", () => { const a: any[] = new Array(N); a[7] = 8; return a; }],
  ["two far stores", () => { const a: any[] = [1, , 3]; a[N >> 1] = 5; a[N - 1] = 9; return a; }],
  ["objects", () => { const a: any[] = [{ k: 1 }]; a[N] = [2, 3]; return a; }],
  ["dense control", () => [1, 2, 3, "x", { y: 1 }]],
  ["holes control", () => [1, , , 4, , 6]],
  ["empty control", () => []],
];
for (const [name, mk] of cases) {
  const a = mk();
  console.log("== " + name);
  console.log(a);
  console.log(util.inspect(a));
  console.log(util.format("%s|%O", "fmt", a));
  console.log(util.format("%j", a).length);
  console.log(util.inspect([a, a.length]));
  console.log(util.format("%o", a.length < 100 ? a : [a.length]));
}

// An inspect hook that shortens the Array it is printed from: the walk must
// re-read the bounds after every element.
{
  const a: any[] = [1, 2, 3, 4, 5];
  const shrink = { [util.inspect.custom]() { a.length = 1; return "S"; } };
  a[1] = shrink;
  console.log(util.inspect(a));
  console.log(a.length);
}
{
  const a: any[] = [];
  const shrink = { [util.inspect.custom]() { a.length = 0; return "S"; } };
  a[0] = shrink;
  a[N] = "gone";
  console.log(util.inspect(a));
  console.log(a.length);
}
