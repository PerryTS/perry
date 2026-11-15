// A generator object has the members it inherits: `[Symbol.iterator]`
// returns the generator itself and the iterator helpers pull its values.
// In the whole-program solve a generator object had `next`, `return` and
// `throw` only, so `gen().map(f)` ran nothing and `g[Symbol.iterator]` read
// no value: `f`, or the generator body stepped through
// `g[Symbol.iterator].call(g).next()`, was called inactive while it ran
// (check mode `PERRY_VERIFY_FACTS=1`: "inactive body: body entered"; with
// `PERRY_FACTS_DCE=1` the removed body trapped).
function* gen(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}

console.log("map:", [...gen().map((x) => x * 2)].join("|"));
console.log("filter:", [...gen().filter((x) => x > 1)].join("|"));
console.log("flatMap:", [...gen().flatMap((x) => [x, x * 10])].join("|"));
let sum = 0;
gen().forEach((x) => {
  sum += x;
});
console.log("forEach:", sum);
console.log("some:", gen().some((x) => x === 2), "every:", gen().every((x) => x > 1));
console.log("find:", gen().find((x) => x > 1));
console.log("reduce:", gen().reduce((a, x) => a + x, 10));
console.log("take:", gen().take(2).toArray().join("|"));
console.log("drop-map:", gen().drop(1).map((x) => x + 100).toArray().join("|"));
console.log("chain:", [...gen().filter((x) => x >= 2).take(1)].join("|"));

// What the generator yields is what a helper's callback receives.
function* fns(): Generator<() => string> {
  yield () => "yielded-a";
  yield () => "yielded-b";
}
const called: string[] = [];
fns().forEach((f) => {
  called.push(f());
});
console.log("callback-values:", called.join("|"));
console.log("toArray-values:", fns().toArray()[1]());
for (const f of gen().map((x) => () => "mapped-" + x)) console.log("map-values:", f());

// The generator is its own iterator.
let steps = 0;
function* counted(): Generator<number> {
  steps++;
  yield 10;
  steps++;
  yield 20;
}
const g1: any = counted();
console.log("self:", g1[Symbol.iterator]() === g1, g1[Symbol.iterator]().next().value, steps);

// The method read as a value, then called on the generator (the shape of
// esbuild's `__yieldStar` and tslib's `__values`).
function drive(g: any): number {
  const f = g[Symbol.iterator];
  const it = f.call(g);
  return it.next().value;
}
steps = 0;
console.log("value-call:", drive(counted()), steps);

const g2: any = counted();
const next = g2.next;
steps = 0;
console.log("next-value-call:", next.call(g2).value, next.call(g2).value, steps);

// Controls: the protocol itself.
steps = 0;
for (const x of counted()) sum += x;
console.log("for-of:", sum, steps, [...counted()].length);
