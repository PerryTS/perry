// #3430 — util.debug is an alias of util.debuglog: returns a logger function.
// With NODE_DEBUG unset, the returned logger is a no-op returning undefined.
import * as util from "node:util";

console.log(typeof util.debug);
const log = util.debug("mysection");
console.log(typeof log);
console.log(String(log("hello")));
