import * as util from "node:util";
console.log(typeof util.debug);
const log = util.debug("mysection");
console.log(typeof log);
console.log(String(log("hello")));
