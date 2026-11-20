// The whole-program facts carry a copy's holes from its source: `slice`
// keeps exactly the source's holes, `toReversed`/`with`/`toSpliced` read a
// hole as `undefined`, and an array-like receiver may lack any index. A copy
// of a hole-free Number array stays a Number array, so its elements reach a
// Number-only parameter.
function twice(x: number): number { return x * 2; }

const dense: number[] = [];
for (let i = 0; i < 8; i++) dense.push(i * 3 - 4);
let sum = 0;
for (const x of dense.slice()) sum += twice(x);
for (const x of dense.toReversed()) sum += twice(x);
for (const x of dense.with(2, 100)) sum += twice(x);
for (const x of dense.toSpliced(1, 2, 7, 8, 9)) sum += twice(x);
console.log(sum);

const holey: number[] = [1, 2, 3];
holey[6] = 7;
const kept = holey.slice();
console.log(kept.length, 4 in kept, kept[4], kept.join("|"));
for (const x of kept) console.log("slice", x, typeof x);
for (const x of holey.toReversed()) console.log("rev", x, typeof x);
for (const x of holey.with(0, 5)) console.log("with", x, typeof x);
for (const x of holey.toSpliced(0, 1)) console.log("spliced", x, typeof x);

const like = { length: 3, 0: 10, 2: 30 };
const fromLike = Array.prototype.slice.call(like) as number[];
console.log(fromLike.length, 1 in fromLike, fromLike.join("|"));
for (const x of fromLike) console.log("like", x, typeof x);

const grown = dense.slice(0, 3);
grown[5] = 1;
for (const x of grown) console.log("grown", x, typeof x);
