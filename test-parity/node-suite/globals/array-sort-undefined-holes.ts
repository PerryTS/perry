// Array.prototype.sort with a comparator follows ECMAScript
// SortIndexedProperties + CompareArrayElements: `undefined` elements sort to
// the very end (after every defined element) and the comparator is NEVER
// invoked for them; array holes are excluded from the sort and trail the
// result as holes. Previously the comparator path fed undefined/holes
// straight to the user comparator, so `(a ?? 0) - (b ?? 0)` ranked
// `undefined` as 0 and placed it first. `toSorted` inherits the same rules.

function show(label: string, value: any) {
  console.log(label + " = " + value);
}

// undefined always trails, regardless of comparator direction.
show("asc", JSON.stringify([3, 1, undefined, 2].sort((x, y) => (x ?? 0) - (y ?? 0))));
show("desc", JSON.stringify([1, undefined, 3, 2].sort((x, y) => (y ?? 0) - (x ?? 0))));
show("undef first in", JSON.stringify([undefined, 5, 1].sort((x, y) => (x ?? 0) - (y ?? 0))));
show("two undef", JSON.stringify([5, undefined, 1, undefined, 3].sort((x, y) => (x ?? 0) - (y ?? 0))));
show("all undef", JSON.stringify([undefined, undefined, undefined].sort((x, y) => 1)));

// holes are excluded and trail as holes after defined + undefined values.
show("hole", JSON.stringify([3, undefined, , 1].sort((x, y) => (x ?? 0) - (y ?? 0))));
show("holes only", JSON.stringify([, , ,].sort((x, y) => 1)));
show("hole+def", JSON.stringify([3, , 1, , 2].sort((x, y) => x - y)));
show("hole keeps length", (() => {
  const a = [3, , 1];
  a.sort((x, y) => x - y);
  return a.length + " idx1=" + (1 in a) + " idx2=" + (2 in a);
})());

// stability preserved with undefined present.
const items = [{ k: 1, i: 0 }, { k: 0, i: 1 }, { k: 1, i: 2 }, { k: 0, i: 3 }];
show("stable", JSON.stringify(items.sort((a, b) => a.k - b.k).map((x) => x.i)));

// large array with scattered undefined (exercises the merge path).
const big: any[] = [];
for (let i = 0; i < 100; i++) big.push(i % 7 === 0 ? undefined : 100 - i);
show("big", JSON.stringify(big.sort((x, y) => (x ?? 1e9) - (y ?? 1e9))));

// toSorted (immutable) inherits the semantics.
show("toSorted", JSON.stringify([3, undefined, 1, 2].toSorted((x, y) => (x ?? 0) - (y ?? 0))));

// regression: plain numeric comparator sorts unchanged.
show("nums asc", JSON.stringify([10, 9, 100, 1, 50].sort((x, y) => x - y)));
show("nums desc", JSON.stringify([10, 9, 100, 1, 50].sort((x, y) => y - x)));
