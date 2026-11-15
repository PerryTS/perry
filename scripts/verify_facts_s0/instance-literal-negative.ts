// Instance nodes: `walk` has an Array parameter, so the facts solve it once
// per call site, and the literal `other` is one Array per instance. `fill`
// pushes onto it through its parameter: the push lands on the instance's
// Array, which only the instance's copy of `other` holds. The facts read a
// binding off its shared node and every instance copy, so `other` has no
// constant length. Under PERRY_FACTS_SABOTAGE=instlit they read the shared
// node alone, whose Array no push reaches: `other.length` is claimed to be
// the constant 3 and would fold. The check-mode witness at the length read
// must report the grown Array ("array constant length").
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}
function walk(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) {
    s += v + other.length;
    fill(other, 1);
  }
  return s + other.length;
}
console.log(walk([1, 4, 7, 10, 13]), walk([]));
