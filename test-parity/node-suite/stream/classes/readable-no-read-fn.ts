import { Readable } from "node:stream";
// new Readable({}) — no read fn — works for purely push-based usage.
// (Calling read() emits 'error' with ERR_METHOD_NOT_IMPLEMENTED only in
// pull mode; here we push, so no error.)
const r = new Readable({});
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("out:", out.join(",")));
r.on("error", () => {});
r.push("a");
r.push("b");
r.push(null);
