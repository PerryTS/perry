// #12485: inside a loop region a `+` tree over `o.k.length` leaves folds the
// string lengths with `fadd`. Every receiver that is not a string must keep
// ordinary `.length` semantics, in source order, exactly once per read.

function sumLengths(n: number, o: any): number {
  let h = 0.0;
  for (let k = 0; k < n; k++) {
    o.d = k;
    h += o.a.length + o.b.length + o.c.length + o.e.length;
  }
  return h;
}

// SSO (ASCII and non-ASCII), heap strings, astral code points.
console.log(sumLengths(3, { a: "a", b: "bb", c: "cccc", e: "eeeeeeee", d: 0 }));
console.log(sumLengths(2, { a: "é", b: "日本", c: "😀x", e: "long heap string ✓", d: 0 }));
console.log(sumLengths(2, { a: "", b: "", c: "", e: "", d: 0 }));

// Arrays read their length word; array-likes take the generic arm.
console.log(sumLengths(2, { a: [1, 2, 3], b: "bb", c: { length: 7 }, e: "e", d: 0 }));
class Stack extends Array<number> {}
const st = new Stack();
st.push(1, 2);
const holey: number[] = [];
holey[9] = 1;
console.log(sumLengths(2, { a: st, b: holey, c: [], e: new Array(4), d: 0 }));
function growing(n: number, o: any): number {
  let h = 0;
  for (let k = 0; k < n; k++) {
    o.d = k;
    h += o.a.length + o.b.length;
    if (k % 2 === 0) o.a.push(k);
  }
  return h;
}
console.log(growing(6, { a: [], b: "xy", d: 0 }));

// A non-string leaf mid-tree: the generic arm must not observe a read twice.
let gets = 0;
const counted = {
  get length() {
    gets++;
    return 10;
  },
};
console.log(sumLengths(3, { a: "x", b: counted, c: "yy", e: "z", d: 0 }), gets);

// #10904's shape: an earlier conversion mutates what a later leaf reads.
const mut: any = { a: "ab", b: "", c: "cc", e: "e", d: 0 };
mut.b = {
  get length() {
    return {
      valueOf() {
        mut.c = "changed!";
        return 1;
      },
    };
  },
};
console.log(sumLengths(1, mut), mut.c);

// A length that is not a number concatenates.
console.log(sumLengths(1, { a: "q", b: { length: "s" }, c: "r", e: "t", d: 0 }));

// Missing keys and undefined values throw like node.
try {
  sumLengths(1, { a: "a", b: undefined, c: "c", e: "e", d: 0 });
} catch (e) {
  console.log((e as Error).constructor.name);
}

// The receiver changes shape mid-loop: the region re-checks.
function reshape(n: number, o: any): number {
  let h = 0;
  for (let k = 0; k < n; k++) {
    o.d = k;
    if (k === 2) o.extra = 1;
    if (k === 3) o.a = [1, 2];
    h += o.a.length + o.b.length;
  }
  return h;
}
console.log(reshape(6, { a: "abc", b: "de", d: 0 }));

// #12484: a key added past the receiver's inline slots, then overwritten.
function addThenStore(n: number, o: any): number {
  let h = 0;
  for (let k = 0; k < n; k++) {
    o.z = k;
    h += o.z;
  }
  return h;
}
const wide = { a: 1, b: 2, c: 4, e: 8, d: 16 };
console.log(addThenStore(10, wide), Object.keys(wide).join(","), (wide as any).z);
const frozenLater: any = { a: 1, b: 2, c: 4, e: 8, d: 16 };
addThenStore(3, frozenLater);
Object.freeze(frozenLater);
try {
  (function (o: any) {
    "use strict";
    for (let k = 0; k < 3; k++) o.z = 100 + k;
  })(frozenLater);
} catch (e) {
  console.log((e as Error).constructor.name, frozenLater.z);
}
const objs: any[] = [];
for (let i = 0; i < 8; i++) objs.push({ a: "a", b: i, c: 4, e: 8, d: 16 });
let acc = 0;
for (let k = 0; k < 64; k++) {
  const o = objs[k & 7];
  o.z = k;
  acc += o.z;
}
console.log(acc, objs[3].z);
