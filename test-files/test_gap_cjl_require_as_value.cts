// CJL: `require` held as a value and called with a literal goes through the
// module's `require` function, whose arm is the same edge as a direct call:
// one evaluation, the same exports.
console.log("start");
const r = require;
const viaValue = r("./cjl_value/dep.cjs");
console.log("value:", viaValue.name);
console.log("same:", require("./cjl_value/dep.cjs") === viaValue);
console.log("module.require:", module.require("./cjl_value/dep.cjs") === viaValue);
