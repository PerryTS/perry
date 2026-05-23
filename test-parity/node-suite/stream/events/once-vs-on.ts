import { PassThrough } from "node:stream";
// once('data', cb) fires exactly once; subsequent emits don't reach it.
const p = new PassThrough();
let onceCalls = 0;
let onCalls = 0;
p.once("data", () => onceCalls++);
p.on("data", () => onCalls++);
p.write("a");
p.write("b");
p.end(() => console.log("once:", onceCalls, "on:", onCalls));
