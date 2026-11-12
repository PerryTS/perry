// m9: a callee's birth pushed into an accumulator declared outside the loop: alive at the caller's iteration end.
function add(acc: any[], i: number): void {
  acc.push({ a: i, b: "w" });
}
function run(n: number): number {
  const acc: any[] = [];
  for (let i = 0; i < n; i++) {
    add(acc, i);
  }
  return acc.length;
}
console.log(run(200));
