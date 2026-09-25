// #10714: a `.length` read through a receiver the front end cannot type is
// answered from a live plain Array's GC header at the call site, instead of
// calling out to the inline-cache miss handler.
//
// That is only sound if the header answer is the one the runtime computes for
// EVERY array state that can reach the site, and if everything that is not a
// live plain Array still takes the tower unchanged. So this reads `.length`
// through one generic call site on arrays that grew past their capacity while
// a stale alias kept the old head (a forwarding stub), sparse arrays, arrays
// whose length was shrunk, grown, shifted and frozen, holey and derived
// arrays, an Array subclass, and — at the same site — the receivers the arm
// must refuse.

type Box = { payload: any; label: string };

function boxed(value: any): Box {
  return { payload: value, label: "box" };
}

// `box.payload` is `any`, so `.length` here lowers through the generic tower.
function readLength(value: any): any {
  const box = boxed(value);
  return box.payload.length;
}

function show(label: string, value: any): void {
  const length = readLength(value);
  console.log(label, String(length), typeof length);
}

show("empty", []);
show("literal", [1, 2, 3]);
show("nested", [[1], [2, 3]]);

// Growth past the initial capacity moves the array; the alias still holds the
// old head, which is now a forwarding stub whose length word is gone.
const grown: any = [];
const stale: any = grown;
for (let i = 0; i < 100; i++) grown.push(i);
show("grown", grown);
show("stale alias", stale);
const staleAgain: any = grown;
for (let i = 0; i < 1000; i++) grown.push(i);
show("grown twice", grown);
show("stale alias twice", staleAgain);
show("first alias", stale);
// Read it again: the first read through a multi-edge chain compresses it.
show("first alias again", stale);

// Grown THROUGH a field: the field keeps the old head until a collection, and
// a loop that allocates nothing never collects.
class Holder {
  items: any = [];
}
const holder: any = new Holder();
for (let i = 0; i < 40; i++) holder.items.push(i);
let fieldTotal = 0;
for (let i = 0; i < 1000; i++) fieldTotal += readLength(holder.items);
show("field grown", holder.items);
console.log("field total", fieldTotal);
holder.items.length = 5;
show("field truncated", holder.items);

// Sparse: a far index sets a logical length far above the dense capacity.
const sparse: any = [];
sparse[1000] = "far";
show("sparse", sparse);
sparse.length = 3;
show("sparse truncated", sparse);

const shrunk: any = [1, 2, 3, 4, 5, 6];
shrunk.length = 2;
show("shrunk", shrunk);
shrunk.length = 10;
show("extended", shrunk);

const shifted: any = [1, 2, 3, 4];
shifted.shift();
shifted.shift();
show("shifted", shifted);
shifted.unshift(9);
show("unshifted", shifted);
shifted.pop();
show("popped", shifted);

show("frozen", Object.freeze([1, 2, 3]));
show("holey", new Array(5));
show("array of", Array.of(7, 8));
show("from", Array.from({ length: 4 }, (_, i) => i));
show("map", [1, 2, 3].map((x) => x * 2));
show("filter", [1, 2, 3, 4].filter((x) => x % 2 === 0));
show("split", "a,b,c,d".split(","));
show("keys", Object.keys({ a: 1, b: 2, c: 3 }));
show("json", JSON.parse("[1,2,3,4,5]"));
show("json nested", JSON.parse('{"xs":[1,2]}').xs);

class Stack extends Array<number> {}
const stack = new Stack();
stack.push(1, 2, 3);
show("subclass", stack);

// The same site keeps refusing what is not a live plain Array.
show("string", "hello");
show("array-like", { length: 3, 0: "a" });
show("array-like string", { length: "three" });
show("no length", { other: 1 });
show("typed array", new Float64Array(6));
show("Map", new Map([[1, 2]]));
show("Set", new Set([1, 2, 3]));
show("function", (a: any, b: any, c: any) => [a, b, c]);

// One site that sees both kinds, while the array changes under it: every read
// must see the current length, never a value from an earlier read.
function lengths(items: any[]): string {
  const out: string[] = [];
  for (let i = 0; i < items.length; i++) {
    out.push(String(readLength(items[i])));
  }
  return out.join(",");
}
const live: any = [];
const mixed: any[] = [live, { length: 42 }, live, "abc", live];
for (let round = 0; round < 3; round++) {
  live.push(round);
  console.log("mixed", round, lengths(mixed));
}

let total = 0;
const counted: any = [];
for (let i = 0; i < 5000; i++) {
  counted.push(i);
  total += readLength(counted);
}
console.log("running total", total);

for (const value of [null, undefined]) {
  try {
    readLength(value);
    console.log("nullish", String(value), "no throw");
  } catch (error) {
    const caught = error as Error;
    console.log(
      "nullish",
      String(value),
      caught.constructor.name + ": " + caught.message,
    );
  }
}
