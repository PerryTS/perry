import { Readable } from "node:stream";
import { once } from "node:events";
// events.once(emitter, eventName) returns a Promise that resolves with
// the event args. Useful with streams to await a single event.
const r = Readable.from(["x"]);
r.on("data", () => {});
const args: any[] = (await once(r, "end")) as any[];
console.log("args length:", args.length);
console.log("typeof:", typeof args);
