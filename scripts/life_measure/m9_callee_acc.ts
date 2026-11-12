// m9: a callee's birth pushed into an accumulator declared outside the loop: alive at the caller's iteration end.
// `add` has m8's `make` body shape (a JSON.stringify call) so it stays a real callee, not inlined into `run`.
function add(acc: any[], i: number): number {
  const o = { a: i, b: "w" };
  acc.push(o);
  return JSON.stringify(o).length;
}
function run(n: number): number {
  const acc: any[] = [];
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += add(acc, i);
  }
  return acc.length + s;
}
console.log(run(200));
