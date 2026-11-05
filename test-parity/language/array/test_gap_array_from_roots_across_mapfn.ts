// Array.from / Array.of keep the result, the source, the callback, the
// snapshot and the iterator rooted while the mapping callback allocates
// enough to run evacuating minors. A raw copy of the result held across the
// callback stored every later element into from-space: the result came back
// with holes and the stores corrupted whatever the nursery held there next
// (OpenCode TUI: `[gc-pin-latch] FATAL` in a later minor).
function churn(i: number): { i: number; n: number; tag: string } {
  const junk: { j: number; s: string }[] = [];
  for (let j = 0; j < 3000; j++) junk.push({ j, s: "x" + j });
  return { i, n: junk.length, tag: "t" + i };
}
function check(name: string, r: any[], n: number): void {
  let bad = 0;
  for (let i = 0; i < n; i++) {
    const o = r[i];
    if (!o || o.i !== i || o.n !== 3000 || o.tag !== "t" + i) bad++;
  }
  console.log(name, r.length, bad);
}
const N = 400;
const arrayLike: any = { length: N };
check("array-like", Array.from(arrayLike, (_: any, i: number) => churn(i)), N);
const set = new Set<number>();
for (let i = 0; i < N; i++) set.add(i);
check("set", Array.from(set, (v: number) => churn(v)), N);
function* gen(): Generator<number> {
  for (let i = 0; i < N; i++) yield i;
}
check("generator", Array.from(gen(), (v: number) => churn(v)), N);
const live = Array.from({ length: N }, (_: any, i: number) => i);
check("array", Array.from(live, (v: number) => churn(v)), N);
check("string", Array.from("a".repeat(N), (_: string, i: number) => churn(i)), N);
class Sub extends Array<any> {}
check("subclass", Sub.from(arrayLike, (_: any, i: number) => churn(i)), N);
const objs: any[] = [];
for (let i = 0; i < N; i++) objs.push(churn(i));
check("of", Array.of(...objs), N);
