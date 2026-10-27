// CJL: module.children holds each required child once per (parent, child)
// pair, in first-require order; a child cached by one parent is still pushed
// to a second parent that requires it; `parent` is the first requirer.
const path = require("path");
const p1 = require("./cjl_children/p1.cjs");
const p2 = require("./cjl_children/p2.cjs");
require("./cjl_children/p1.cjs");
console.log("entry:", module.children.map((m) => path.basename(m.id)).join(","));
console.log("p1:", p1.children().join(","));
console.log("p2:", p2.children().join(","), p2.name);
console.log("c.parent:", p2.cParent());
