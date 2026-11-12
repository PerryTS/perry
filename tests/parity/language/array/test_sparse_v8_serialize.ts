// v8.serialize / v8.deserialize of a holey or sparse Array: holes stay
// absent (`i in clone` false), far elements and the length survive, trailing
// holes keep the length. structuredClone is the control (it was right at the
// top level). Dense Arrays round-trip as before.
import * as v8 from "node:v8";
const N = 3000000;
const cases: [string, () => any[]][] = [
  ["far store", () => { const a: any[] = []; a[N] = 1; return a; }],
  ["dense head, far tail", () => { const a: any[] = ["a0", 11]; a[N] = "far"; return a; }],
  ["length set", () => { const a: any[] = [1, 2]; a.length = N; return a; }],
  ["new Array(N)", () => { const a: any[] = new Array(N); a[7] = { k: 8 }; return a; }],
  ["holes", () => [1, , 3, , ]],
  ["trailing holes", () => { const a: any[] = [1, 2]; a.length = 5; return a; }],
  ["nested", () => [[1, , 2], { x: [, "y"] }]],
  ["dense control", () => [1, "two", { three: 3 }, [4]]],
  ["empty control", () => []],
];
function sig(b: any): string {
  return b.length + " own=" + JSON.stringify(Object.keys(b).slice(0, 5)) + " 0in=" + (0 in b) + " 1in=" + (1 in b)
    + " last=" + JSON.stringify(b[b.length - 1]) + " j=" + JSON.stringify(b.slice(0, 4));
}
for (const [name, mk] of cases) {
  const a = mk();
  console.log(name, "v8", sig(v8.deserialize(v8.serialize(a))));
  console.log(name, "v8 nested", sig(v8.deserialize(v8.serialize({ a })).a));
  console.log(name, "sc", sig(structuredClone(a)));
}
