// m2: the birth is read inside the iteration only: 100% dead at the iteration.
function sum(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: "y" };
    s += JSON.stringify(o).length;
  }
  return s;
}
console.log(sum(200));
