import { Readable } from "node:stream";
import { once } from "node:events";
// events.once(stream, 'end') returns a Promise<args>.
const r = Readable.from(["x"]);
r.on("data", () => {});
const args = await once(r, "end");
console.log("args is array:", Array.isArray(args));
console.log("args length:", (args as any[]).length);
