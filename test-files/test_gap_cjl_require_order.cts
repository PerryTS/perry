// CJL (design 2026-10-26 section 4.1): a CommonJS module runs when its
// require call runs, after the requirer statements that precede it. Three
// modules, each with a side effect before its require. Node prints
// entry/a/b interleaved in call order; a hoisted-import lowering runs a and
// b before the entry body.
//
// `.cts` keeps the CommonJS goal under Node (see test_gap_10735's header).
console.log("entry: start");
const a = require("./cjl_order/a.cjs");
console.log("entry: a says", a.name);
const again = require("./cjl_order/a.cjs");
console.log("entry: same exports", again === a);
function late() {
  return require("./cjl_order/b.cjs").name;
}
console.log("entry: late", late());
