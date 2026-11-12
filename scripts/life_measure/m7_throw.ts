// m7: a birth, then a throw across two frames: its return is decided late, and it is dead.
function inner(i: number): number {
  const o = { a: i, b: "e" };
  if (JSON.stringify(o).length > 0) throw new Error("x");
  return 0;
}
function mid(i: number): number {
  return inner(i) + 1;
}
function run(n: number): number {
  let c = 0;
  for (let i = 0; i < n; i++) {
    try {
      mid(i);
    } catch (e) {
      c++;
    }
  }
  return c;
}
console.log(run(100));
