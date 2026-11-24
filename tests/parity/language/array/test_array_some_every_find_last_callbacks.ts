// some, every, findLast and findLastIndex call their callback with (element, index,
// array) and answer as the spec says, for an empty array too.
const a = [1, 2, 3];
const k = 2;
console.log(a.some((v) => v > 2), a.some((v, i) => v < i), a.every((v) => v > 0), a.every((v, i) => v > i + 1));
console.log(a.findLast((v) => v < 3), a.findLastIndex((v) => v < 3), a.findLast((v) => v > 9), a.findLastIndex((v) => v > 9));
console.log([].some((v) => true), [].every((v) => false), a.some((v) => v === k), a.findLast((v, i, arr) => arr.length === 3 && i === 0));
