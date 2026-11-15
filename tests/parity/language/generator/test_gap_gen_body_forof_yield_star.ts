// The inner generator delegates with yield*; the outer body's for-of steps it.
function* leaf(n: number): Generator<number> { for (let i = 0; i < n; i++) yield i; return 100 + n; }
function* inner(): Generator<number> {
  const r = yield* leaf(2);
  yield r;
  yield* [7, 8];
  yield* leaf(1);
}
function* mid(it: Generator<number>): Generator<string> { for (const v of it) yield "m" + v; }
console.log([...mid(inner())].join(","));
// The outer body itself uses yield* over a generator, beside a for-of.
function* both(): Generator<string> { yield* mid(inner()); for (const v of leaf(3)) yield "b" + v; }
console.log([...both()].join(","));
// Early exit from the mid loop while the inner is inside its yield*.
const log: string[] = [];
function* guarded(): Generator<number> { try { yield* leaf(5); } finally { log.push("guarded-finally"); } }
function* firstTwo(it: Generator<number>): Generator<number> { let k = 0; for (const v of it) { yield v; if (++k === 2) return; } }
console.log([...firstTwo(guarded())].join(","), log.join(","));
