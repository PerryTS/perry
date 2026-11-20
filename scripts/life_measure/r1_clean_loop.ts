// LIFE I3 fixture: every iteration of run's loop is clean (an object literal
// and runtime strings built and dropped inside it).
function sum(o: { a: number; b: number }): number {
  return o.a + o.b;
}
function run(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: i * 2 };
    const s = ("item-" + i).repeat(4);
    total += sum(o) + s.length;
  }
  return total;
}
console.log(run(200));
