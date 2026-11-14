// LIFE I3 fixture: runtime-only garbage (string concatenation, no object
// literal): every iteration of run's loop is clean and its bytes are counted.
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const s = ("abcdefghij" + i).repeat(5);
    const u = s.toUpperCase() + s;
    t += u.length;
  }
  return t;
}
console.log(run(200));
