// `structuredClone` (the message serializer) of a sparse Array: a fresh
// `new Array(n)` above the dense limit, a long `length`, a far store. The
// clone has the length, the present elements and holes; the serializer's
// bulk copy of a numeric Array must not take `length` slots of a short store.
// Negative controls: dense numeric and mixed Arrays.
function sig(a: any[]): string {
  let nums = 0, sum = 0, holes = 0, other = 0;
  for (let i = 0; i < a.length; i++) {
    if (!(i in a)) holes++;
    else if (typeof a[i] === "number") { nums++; sum += a[i]; }
    else other++;
  }
  return "len=" + a.length + " nums=" + nums + " sum=" + sum + " holes=" + holes + " other=" + other;
}
const MK: [string, () => any[]][] = [
  ["new Array(3000000)", () => new Array(3000000)],
  ["new Array(1200000) + element", () => { const a: any[] = new Array(1200000); a[1] = 7; return a; }],
  ["new Array<number>(2000000)", () => { const a = new Array<number>(2000000); a[0] = 1.5; a[5] = 2.5; return a; }],
  ["far store", () => { const a: any[] = []; a[3000000] = 1; return a; }],
  ["numbers + far store", () => { const a: number[] = [1.5, 2.5]; a[2000000] = 4; return a; }],
  ["length set", () => { const a: number[] = [1, 2, 3]; a.length = 1500000; return a; }],
  ["new Array(100) control", () => new Array(100)],
  ["dense numbers control", () => { const a: number[] = []; for (let i = 0; i < 5000; i++) a.push(i * 0.5); return a; }],
  ["dense mixed control", () => [1, "s", { k: 1 }, , 4.5]],
];
for (const [name, mk] of MK) {
  let r: string;
  try {
    const a = mk();
    const c = structuredClone(a);
    r = sig(c) + " | nested " + sig(structuredClone({ a }).a) + " | same=" + (c === a);
  } catch (e: any) {
    r = "throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(name + " -> " + r);
}
// The guarded numeric read of a fresh sparse Array.
function sum(a: number[]): number { let s = 0; for (let i = 0; i < a.length; i++) { const v = a[i]; if (v === v) s += v || 0; } return s; }
const fresh = new Array<number>(2500000);
console.log("sum of a fresh sparse Array", sum(fresh), fresh.indexOf(1.5), fresh.includes(0));
