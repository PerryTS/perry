// m8: a birth in a callee of a loop body (no loop in the callee): dead at the caller's iteration end.
function make(i: number): number {
  const o = { a: i, b: "w" };
  return JSON.stringify(o).length;
}
function run(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += make(i);
  }
  return s;
}
console.log(run(200));
