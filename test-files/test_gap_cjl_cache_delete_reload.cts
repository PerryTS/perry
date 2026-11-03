// CJL (design 2026-10-26 section 4.4): `delete require.cache[P]; require(P)`
// runs P again. The delete is the program's only loader capability (the key
// is computed without `require`), so it alone makes the program not closed
// and every literal require takes the escape form that reads the live cache.
const path = require("path");
const P = path.join(__dirname, "cjl_reload", "counter.cjs");
const first = require("./cjl_reload/counter.cjs");
console.log("first:", first.loads);
console.log("cached:", require("./cjl_reload/counter.cjs") === first);
delete require.cache[P];
const second = require("./cjl_reload/counter.cjs");
console.log("second:", second.loads, second === first);
console.log("third is second:", require("./cjl_reload/counter.cjs") === second);
