// callee recurses with its parameter and holds it in its frame across the death of the outer claim.
// Sabotage fate:walk#0 at the inner call -> the argument is still live in an outer frame (native_stack).
function walk(o: { v: number }, n: number): number {
  if (n === 0) return o.v;
  const r = walk({ v: o.v + 1 }, n - 1);
  return r + o.v;
}
console.log(walk({ v: 1 }, 4));
