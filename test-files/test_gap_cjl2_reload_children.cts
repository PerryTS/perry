// CJL2: Node's `updateChildren` links a fresh Module into its requirer's
// `children` without a scan and a cached one only when it is not there yet,
// so after `delete require.cache[T]; require(T)` the requiring module holds
// the first record and the new one, and a repeat require adds nothing. The
// static edge's link flag is the record it linked, so a new record (a reload)
// links again and the same record never does.
const path = require("path");
const T = path.join(__dirname, "cjl2_reload", "t.cjs");
const first = require("./cjl2_reload/t.cjs");
require("./cjl2_reload/t.cjs");
console.log("children:", module.children.length, module.children[0].exports === first, first.parentIsEntry);
delete require.cache[T];
const second = require("./cjl2_reload/t.cjs");
console.log("reloaded:", second.loads, second !== first, second.parentIsEntry);
console.log("after reload:", module.children.length, module.children[1] !== module.children[0], module.children[1].exports === second, module.children[1].parent === module);
require("./cjl2_reload/t.cjs");
console.log("repeat:", module.children.length, require("./cjl2_reload/t.cjs") === second);
