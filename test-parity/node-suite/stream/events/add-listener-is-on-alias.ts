import { Readable } from "node:stream";
// addListener is an alias of on; both should be defined and identical.
const r = new Readable({ read() {} });
console.log("on typeof:", typeof r.on);
console.log("addListener typeof:", typeof r.addListener);
console.log("identical:", r.on === r.addListener);
