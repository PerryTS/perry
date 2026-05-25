import { PassThrough } from "node:stream";
// PassThrough — data written appears on the readable side unchanged.
const pt = new PassThrough();
const out: string[] = [];
pt.on("data", (c) => out.push(String(c)));
pt.on("end", () => console.log("passed through:", out.join(",")));
pt.write("a");
pt.write("b");
pt.end("c");
