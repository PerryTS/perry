// Operations that take an array out of the plain representation
// (DESIGN-p4l1 §5.4 fixture 5): `delete`, an accessor element, a frozen
// array. Each is followed by reads of the same binding in a loop.

function read(a: number[], n: number): string {
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    const k = i;
    const v = a[k];
    out.push(v === undefined ? "u" : String(v));
  }
  return out.join(",");
}
function total(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = i;
    s += a[k] ?? 100;
  }
  return s;
}

// delete: a hole below the length.
const deleted = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
console.log("before delete", read(deleted, 4), total(deleted, 18));
delete (deleted as any)[1];
console.log("after delete", read(deleted, 4), total(deleted, 18), 1 in deleted, deleted.length);

// An accessor element.
const accessor = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
let reads = 0;
Object.defineProperty(accessor, 0, {
  get() {
    reads++;
    return 40 + reads;
  },
  configurable: true,
});
console.log("accessor", read(accessor, 3), total(accessor, 18), reads);

// A non-writable element keeps its value.
const fixed = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
Object.defineProperty(fixed, 2, { value: 30, writable: false });
try {
  (function strict() {
    "use strict";
    fixed[2] = 99;
  })();
  console.log("no throw");
} catch (e) {
  console.log("fixed store", (e as Error).constructor.name);
}
console.log("fixed", read(fixed, 4), total(fixed, 18));

// A frozen array: a store throws in strict code and changes nothing.
const frozen = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
Object.freeze(frozen);
try {
  (function strict() {
    "use strict";
    frozen[0] = 99;
  })();
  console.log("no throw");
} catch (e) {
  console.log("frozen store", (e as Error).constructor.name);
}
try {
  (function strict() {
    "use strict";
    frozen.push(1);
  })();
  console.log("no throw");
} catch (e) {
  console.log("frozen push", (e as Error).constructor.name);
}
console.log("frozen", read(frozen, 3), total(frozen, 18), frozen.length, Object.isFrozen(frozen));

// A `length` made non-writable.
const pinned = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
Object.defineProperty(pinned, "length", { writable: false });
try {
  (function strict() {
    "use strict";
    pinned[18] = 19;
  })();
  console.log("no throw");
} catch (e) {
  console.log("pinned store", (e as Error).constructor.name);
}
console.log("pinned", read(pinned, 3), total(pinned, 20), pinned.length);
