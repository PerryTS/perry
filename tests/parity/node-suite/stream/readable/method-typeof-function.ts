import { Readable } from "node:stream";
// All instance methods report typeof === "function" (recent #1642/#1643 fix).
const r = new Readable({ read() {} });
console.log("read:", typeof r.read);
console.log("push:", typeof r.push);
console.log("pipe:", typeof r.pipe);
console.log("pause:", typeof r.pause);
console.log("resume:", typeof r.resume);
console.log("destroy:", typeof r.destroy);
console.log("on:", typeof r.on);
console.log("all function:", [r.read, r.push, r.pipe, r.pause, r.resume, r.destroy, r.on].every(f => typeof f === "function"));
