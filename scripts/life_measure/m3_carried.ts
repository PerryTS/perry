// m3: the birth is carried to the next iteration: alive at the iteration, dead at return.
function run(n: number): number {
  let last: any = null;
  let s = 0;
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: "z" };
    s += JSON.stringify(last).length;
    last = o;
  }
  last = null;
  return s;
}
console.log(run(200));
