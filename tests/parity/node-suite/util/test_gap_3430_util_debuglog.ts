import * as util from "node:util";
console.log(typeof util.debuglog);
const log = util.debuglog("mysection");
console.log(typeof log);
console.log(String(log("hello")));
