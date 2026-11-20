// The length floor (wpfacts `solve::arr_len`): `reverse` and `copyWithin`
// keep a point length but make a grown length unknown (Q1); `sort` keeps
// it. Node-exact with the floor on, off and in check mode.

// 4a. A fixed dense array: the length stays a point.
const fixed = [4, 3, 2, 1];
fixed.reverse();
console.log("fixed reverse", fixed.join(","), fixed.length, fixed[0], fixed[3]);
const fixed2 = [1, 2, 3, 4, 5];
fixed2.copyWithin(0, 3);
console.log("fixed copyWithin", fixed2.join(","), fixed2.length, fixed2[1]);

// 4b. A grown holey array: holes move below the old floor.
const grown = [1, 2, 3, 4];
function put(k: number, v: number): void {
  grown[k] = v;
}
put(7, 70);
grown.reverse();
console.log("grown reverse", grown.length, grown[0], grown[1] ?? "hole", grown[3] ?? "hole", grown[7]);
let gs = 0;
for (let i = 0; i < 4; i++) gs += grown[i] ?? 1000;
console.log("grown sum", gs);

const grown2 = [1, 2, 3, 4];
function put2(k: number, v: number): void {
  grown2[k] = v;
}
put2(6, 60);
grown2.copyWithin(0, 4);
console.log("grown copyWithin", grown2.length, grown2[0] ?? "hole", grown2[1] ?? "hole", grown2[2], grown2[3]);

// A permutation through `.call`.
const viaCall = [1, 2, 3];
function put3(k: number, v: number): void {
  viaCall[k] = v;
}
put3(5, 50);
Array.prototype.reverse.call(viaCall);
console.log("call reverse", viaCall.length, viaCall[0], viaCall[2] ?? "hole");

// 4c. sort with holes: values, then undefined, then holes.
const holey = [3, , 1];
const sorted = holey.sort();
console.log("sort holey", sorted.length, sorted[0], sorted[1], sorted[2] ?? "hole", 2 in sorted);
const tail = [5, 1, 4];
function put4(k: number, v: number): void {
  tail[k] = v;
}
put4(6, 0);
tail.sort((x, y) => x - y);
console.log("sort grown", tail.length, tail.join(","), tail[0], tail[3], tail[4] ?? "hole");
