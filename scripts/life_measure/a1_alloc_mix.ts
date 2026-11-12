// a1: the allocation census (PERRY_LIFE_ALLOC) on a known mix: object
// literals at a census site (claimed at birth) and runtime-allocated strings
// of known length (no census site). At one sample per byte the sampled
// covered / uncovered bytes must equal the exact ones.
function mix(n: number): number {
  let s = 0;
  const keep: { a: number; b: number }[] = [];
  for (let i = 0; i < n; i++) {
    const o = { a: i, b: i + 1 };
    keep.push(o);
  }
  for (const o of keep) s += o.a + o.b;
  for (let i = 0; i < n; i++) {
    const t = "x".repeat(100 + (i % 7));
    s += t.length;
  }
  return s;
}
console.log(mix(300));
