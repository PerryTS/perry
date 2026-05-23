// process.hrtime() returns a [seconds, nanoseconds] integer tuple; the diff
// form is non-negative.
const t = process.hrtime();
console.log("is array:", Array.isArray(t));
console.log("length 2:", t.length === 2);
console.log("ints:", Number.isInteger(t[0]) && Number.isInteger(t[1]));
const d = process.hrtime(t);
console.log("diff non-negative:", d[0] >= 0);
