// #7576: `ITERATOR_HELPER_CLASS_ID` collided with `STRING_ITERATOR_CLASS_ID`
// (both 0xFFFF_0009), so every helper object dispatched as a String iterator:
// `Iterator.from(x)` was exhausted before its first step and every combinator
// returned `undefined`.
//
// `test_gap_iterator_helpers_2874.ts` covers the inline-chain forms. This file
// covers the shapes that file does not: a helper STORED in a local and stepped
// with `.next()` by hand, and every source kind (bare `{ next() }`, array
// iterator, Map iterator, Set iterator, string iterator, generator).

function makeCounter(n: number): Iterator<number> {
  let i = 0;
  return {
    next(): IteratorResult<number> {
      return i < n ? { value: i++, done: false } : { value: undefined, done: true };
    },
  };
}

function* gen(n: number): Generator<number, void, undefined> {
  for (let k = 0; k < n; k++) yield k;
}

function drain(it: { next(): { value: unknown; done?: boolean } }): unknown[] {
  const out: unknown[] = [];
  let s = it.next();
  let guard = 0;
  while (!s.done && guard++ < 50) {
    out.push(s.value);
    s = it.next();
  }
  return out;
}

// --- stored helper, hand-stepped: the shape that was silently empty ---
const a = Iterator.from(makeCounter(4));
console.log("A", JSON.stringify(drain(a)));

const b = Iterator.from([7, 8, 9][Symbol.iterator]());
console.log("B", JSON.stringify(drain(b)));

// the same object driven directly, WITHOUT the helper (the control case)
console.log("C", JSON.stringify(drain(makeCounter(3))));

const d = Iterator.from(makeCounter(2));
console.log("D", JSON.stringify(d.next()));
console.log("D2", JSON.stringify(d.next()));
console.log("D3", JSON.stringify(d.next()));

// --- every source kind reaches the same helper ---
console.log("E array", JSON.stringify(Iterator.from([1, 2, 3]).toArray()));
console.log("E gen", JSON.stringify(Iterator.from(gen(3)).toArray()));
console.log(
  "E map",
  JSON.stringify(Iterator.from(new Map([["k", 1], ["j", 2]]).values()).toArray()),
);
console.log("E set", JSON.stringify(Iterator.from(new Set([4, 5])).toArray()));
console.log("E string", JSON.stringify(Iterator.from("hi"[Symbol.iterator]()).toArray()));
console.log("E arrayiter", JSON.stringify(Iterator.from([6, 7].values()).toArray()));

// --- combinators return helper objects, not undefined ---
const src = Iterator.from([1, 2, 3, 4, 5, 6]);
const mapped = src.map((x: number) => x * 2);
console.log("F typeof", typeof mapped);
console.log("F chain", JSON.stringify(mapped.filter((x: number) => x % 3 !== 0).toArray()));

console.log("G take", JSON.stringify(Iterator.from(gen(9)).take(3).toArray()));
console.log("G drop", JSON.stringify(Iterator.from(gen(5)).drop(2).toArray()));
console.log(
  "G flatMap",
  JSON.stringify(Iterator.from([1, 2]).flatMap((x: number) => [x, -x]).toArray()),
);

// --- terminal helpers on a stored receiver ---
const t1 = Iterator.from([1, 2, 3, 4]);
console.log("H reduce", t1.reduce((p: number, c: number) => p + c, 100));
const t2 = Iterator.from([1, 2, 3]);
console.log("H reduce-noinit", t2.reduce((p: number, c: number) => p + c));
const t3 = Iterator.from([1, 2, 3]);
console.log("H some", t3.some((x: number) => x === 2));
const t4 = Iterator.from([1, 2, 3]);
console.log("H every", t4.every((x: number) => x > 0));
const t5 = Iterator.from([1, 2, 3]);
console.log("H find", t5.find((x: number) => x > 1));
const t6 = Iterator.from([1, 2, 3]);
let acc = 0;
t6.forEach((x: number) => {
  acc += x;
});
console.log("H forEach", acc);

// --- laziness: `.take` must terminate over an unbounded generator ---
function* naturals(): Generator<number, void, undefined> {
  let i = 0;
  while (true) yield i++;
}
console.log("I lazy", JSON.stringify(Iterator.from(naturals()).map((x: number) => x * 3).take(4).toArray()));

// --- spread drives the helper through the iterator protocol ---
console.log("J spread", JSON.stringify([...Iterator.from([1, 2, 3]).map((x: number) => x + 1)]));

// --- the String iterator, the other half of the collision, is unaffected ---
const si = "abc"[Symbol.iterator]();
console.log("K string-iter", JSON.stringify(si.next()), JSON.stringify(drain(si)));
console.log("K spread", JSON.stringify([..."héllo"]));

// --- Iterator.from on something already a helper hands it back ---
const h = Iterator.from([1, 2]);
console.log("L identity", Iterator.from(h) === h);
