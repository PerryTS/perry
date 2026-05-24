import { WritableStream } from "node:stream/web";
// `new WritableStream()` with no underlying sink should still construct
// — writes are silently dropped (the default sink swallows everything).
const ws = new WritableStream();
const w = ws.getWriter();
await w.write("x");
await w.close();
console.log("locked:", ws.locked);
console.log("constructed:", ws instanceof WritableStream);
