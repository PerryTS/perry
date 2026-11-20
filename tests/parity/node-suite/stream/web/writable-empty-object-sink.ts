import { WritableStream } from "node:stream/web";
// `new WritableStream({})` — empty sink object: writes silently succeed.
const ws = new WritableStream({});
const w = ws.getWriter();
await w.write("a");
await w.write("b");
await w.close();
console.log("constructed:", ws instanceof WritableStream);
console.log("locked after close:", ws.locked);
