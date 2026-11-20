// m1: every birth is stored in a module array: alive at every boundary.
const keep: any[] = [];
function fill(n: number): void {
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: "x" + i };
    keep.push(o);
  }
}
fill(200);
console.log(keep.length);
