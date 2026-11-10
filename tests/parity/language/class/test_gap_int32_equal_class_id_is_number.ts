// #11414: an INT32 number whose value equals a registered class id is a
// number, never the class. Several classes are declared so the small ids
// 1..6 are all registered, then int32 numbers produced at run time are read
// through every generic reader (typeof, concat, template, String, join,
// Object.prototype.toString, arithmetic, indexing, Proxy, callability).
class A { x = 1; }
class B extends A { y = 2; }
class C { static s = 3; }
class D { m(): number { return 4; } }
class E {}
class F {}
const keep: any[] = [A, B, C, D, E, F];

function sources(): any[] {
  const out: any[] = [];
  const s = new Set<number>();
  for (let i = 1; i <= 6; i++) s.add(i);
  for (const v of s) out.push(v);
  const m = new Map<string, number>();
  for (let i = 1; i <= 6; i++) m.set("k" + i, i);
  for (const v of m.values()) out.push(v);
  const arr: any[] = [];
  for (let i = 0; i < 6; i++) arr.push(i + 1);
  out.push(arr.length, arr.indexOf(3), "abcd".length, (7.9 | 0) - 5, [1, 2, 3].findIndex((x) => x === 3));
  return out;
}

async function asyncReturns(): Promise<any[]> {
  async function two(): Promise<number> { await null; return 2; }
  async function one(): Promise<number> { await null; return 1; }
  return [await one(), await two()];
}

function show(label: string, v: any): void {
  let callable = false;
  try { (v as any)(); callable = true; } catch (e) { callable = e instanceof TypeError ? false : true; }
  let proxied = "ok";
  try { new Proxy(v, {}); } catch (e) { proxied = e instanceof TypeError ? "TypeError" : "other"; }
  console.log(
    label, typeof v, "" + v, `${v}`, String(v), [v, v].join("-"),
    Object.prototype.toString.call(v), v + 1, v * 2, JSON.stringify({ v }),
    (v as any)[0], (v as any).name, callable, proxied, v === 2,
  );
}

const vals = sources();
for (let i = 0; i < vals.length; i++) show("s" + i, vals[i]);
asyncReturns().then((xs) => {
  for (let i = 0; i < xs.length; i++) show("a" + i, xs[i]);
  console.log("classes", keep.length, typeof A, typeof B, String(new D().m()), C.s);
});
