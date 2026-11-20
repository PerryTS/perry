// Memory-leak regression: 100,000 iterations of create-and-discard
// closures with both immutable and mutable (boxed) captures. Each
// iteration creates one closure, calls it 3× (so the box is mutated
// 3 times), then drops the closure. RSS must plateau.
//
// Catches regressions where:
//   - Boxed captures (Box<T> heap allocation) leak — every iter
//     would burn 24 bytes of malloc + a HashSet insert that's never
//     removed
//   - Closure environment retains references to outer scope objects
//   - Shadow-stack (v0.5.217-220) frame slot stores leak through
//     PERRY_SHADOW_STACK=1 path
//   - GC misses closures that share an environment with already-dead
//     ones (sweep ordering bug)

function makeCounter(start: number): () => number {
  let n = start;
  return () => {
    n = n + 1;
    return n;
  };
}

function makeMultiplier(factor: number): (x: number) => number {
  // Immutable capture — `factor` is read-only inside the closure.
  return (x: number) => x * factor;
}

let sum = 0;
for (let i = 0; i < 100_000; i++) {
  const c = makeCounter(i);
  const m = makeMultiplier(i % 13);
  sum += c() + c() + c();
  sum += m(7);
}
console.log("done, sum=" + sum);
