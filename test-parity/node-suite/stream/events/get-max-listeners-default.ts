import { Readable } from "node:stream";
import { EventEmitter } from "node:events";
// getMaxListeners() returns Number; default is EventEmitter.defaultMaxListeners (10).
const r = new Readable({ read() {} });
console.log("default:", r.getMaxListeners());
console.log("is number:", typeof r.getMaxListeners() === "number");
console.log("EE default:", EventEmitter.defaultMaxListeners);
