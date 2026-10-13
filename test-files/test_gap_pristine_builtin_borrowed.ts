// #11915: prototype mutation and actual method selection.
function take(a: number, b: number) { return [].slice.call(arguments).join(','); }
console.log(take(11, 22));
const borrowed: any = Array.prototype.slice;
const other: any = {0: 3, 1: 4, length: 2, slice: function () { return [99]; }};
console.log(borrowed.call(other).join(','));
console.log(borrowed.apply(other, [1]).join(','));
const ap: any = Array.prototype;
ap.slice = function () { return [88]; };
console.log([].slice.call(other).join(','));
console.log(borrowed.call(other).join(','));
