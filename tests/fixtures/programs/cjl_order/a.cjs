// CJL: a side effect before the require of b, then one after it.
console.log("a: start");
const b = require("./b.cjs");
console.log("a: b says", b.name);
module.exports = { name: "a" };
