import { Transform } from "node:stream";
import { createGunzip } from "node:zlib";

const stream = createGunzip();
const events: string[] = [];
stream.on("close", () => { events.push("close"); console.log("close event:", stream.destroyed, stream.closed, (stream as any)._closed); });
await new Promise<void>((resolve) => {
  stream.on("error", (error: any) => {
    events.push("error");
    console.log("error:", error.name, error.code);
    console.log(
      "closed:",
      (stream as any)._closed,
      stream.destroyed,
      stream.closed,
    );
    stream.close();
    console.log(
      "close again:",
      (stream as any)._closed,
      stream.destroyed,
      stream.closed,
    );
    resolve();
  });
  stream.end(Buffer.from("invalid"));
});

await new Promise<void>((resolve) => setImmediate(resolve));
console.log("event order:", events.join(","));
stream.destroy();
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("repeat events:", events.join(","));


let complete: any;
const genericEvents: string[] = [];
const generic = new Transform({ destroy(error, callback) { complete = callback; } });
generic.on("error", (error: any) => {
  genericEvents.push("error");
  console.log("generic error:", error.message, generic.destroyed, generic.closed);
});
generic.on("close", () => genericEvents.push("close"));
generic.destroy(new Error("original"));
console.log("generic pending:", generic.destroyed, generic.closed, genericEvents.join(","));
complete(new Error("replacement"));
console.log("generic complete:", generic.destroyed, generic.closed, genericEvents.join(","));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("generic order:", genericEvents.join(","));
