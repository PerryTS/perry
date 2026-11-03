// require.cache is a null-prototype record: its shape says so, so reads find
// nothing on Object.prototype.
const self = require("./fixtures/awb2-static-proto/base.cjs");
const c: any = require.cache;
console.log("1", Object.getPrototypeOf(c) === null, typeof c.toString, typeof c.hasOwnProperty, "toString" in c, typeof self.Base);
c.extra = 1;
console.log("2", Object.getPrototypeOf(c) === null, typeof c.toString, Object.keys(c).includes("extra"));
console.log("3", Object.getPrototypeOf(exports) === Object.prototype, typeof exports.hasOwnProperty, typeof module.exports.toString);
