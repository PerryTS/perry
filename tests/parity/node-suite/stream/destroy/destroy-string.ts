import { Readable } from "node:stream";
// destroy(string) treats the value as the error payload — the error
// listener receives the string (not wrapped in Error).
const r = new Readable({ read() {} });
let got: any = null;
r.on("error", (e: any) => (got = e));
r.destroy("nope" as any);
setImmediate(() => console.log("got:", got));
