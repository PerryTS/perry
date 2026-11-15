// Object.values / Object.entries of an Array: every own element in index
// order (a sparse Array's far elements, which live past its short store,
// included), then the enumerable named keys. The answer does not depend on
// the Array's length. Keys / getOwnPropertyNames / for-in / spread are the
// controls (they were right), and so are small dense Arrays.
const N = 3000000;
const cases: [string, () => any[]][] = [
  ["far store", () => { const a: any[] = []; a[N] = 1; return a; }],
  ["length set", () => { const a: any[] = [5]; a.length = N; a[N - 1] = "z"; return a; }],
  ["new Array(N)", () => { const a: any[] = new Array(N); a[7] = 8; return a; }],
  ["two far stores, reverse order", () => { const a: any[] = [1, 2]; a[N] = "b"; a[N - 5] = "a"; return a; }],
  ["named key", () => { const a: any[] = [1, 2]; a[2000005] = 3; (a as any).prop = "p"; return a; }],
  ["dense 150000", () => { const a: number[] = []; for (let i = 0; i < 150000; i++) a.push(i); return a; }],
  ["holes control", () => [1, , 3, , 5]],
  ["small control", () => { const a: any[] = ["x", "y"]; (a as any).k = 1; return a; }],
];
for (const [name, mk] of cases) {
  const a = mk();
  const v = Object.values(a), e = Object.entries(a);
  let sum = 0; for (const x of v) if (typeof x === "number") sum += x;
  console.log(name, a.length, v.length, sum, JSON.stringify(v.slice(-3)), e.length, JSON.stringify(e.slice(0, 2)), JSON.stringify(e.slice(-2)));
  console.log(" keys", Object.keys(a).length, Object.getOwnPropertyNames(a).length, Object.keys({ ...a }).length);
  let n = 0; for (const _k in a) n++; console.log(" for-in", n);
}
// A getter on an index runs once per call, in index order.
{
  const a: any[] = [1, 2, 3];
  const log: string[] = [];
  Object.defineProperty(a, 1, { get() { log.push("g1"); return 20; }, enumerable: true, configurable: true });
  console.log(JSON.stringify(Object.values(a)), JSON.stringify(Object.entries(a)), log.join(","));
}
