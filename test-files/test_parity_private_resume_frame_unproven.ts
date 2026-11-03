function* values(f: () => number) { yield f(); yield f(); }
// The alias has no closed callback transfer in this slice.
const callback = function () { return 7; };
let total = 0;
for (const x of values(callback)) total += x;
console.log(total);
