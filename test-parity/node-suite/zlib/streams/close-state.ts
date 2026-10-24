import { Transform } from "node:stream";
import { createGzip } from "node:zlib";

const stream = createGzip();
console.log(
  "initial:",
  stream.destroyed,
  stream.closed,
  (stream as any)._closed,
);
console.log("return:", stream.close());
console.log(
  "closed:",
  stream.destroyed,
  stream.closed,
  (stream as any)._closed,
  (stream as any)._handle === null,
);
stream.close();
console.log(
  "idempotent:",
  stream.destroyed,
  stream.closed,
  (stream as any)._closed,
);

// Observe codec-handle state through the inherited accessor's ordinary Get.
const zlibBase = Object.getPrototypeOf(Object.getPrototypeOf(Object.getPrototypeOf(stream)));
const closedGetter = Object.getOwnPropertyDescriptor(zlibBase, "_closed")!.get!;
for (const handle of [undefined, null, false, 0, "", true, 1, "open", {}]) {
  console.log("handle truthiness:", closedGetter.call({ _handle: handle }));
}
console.log("inherited handle:", closedGetter.call(Object.create({ _handle: {} })));
let reads = 0;
const receiver = Object.create(null);
Object.defineProperty(receiver, "_handle", { get() { reads++; return false; } });
console.log("accessor handle:", closedGetter.call(receiver), reads);

// Releasing a codec is distinct from destroying its Transform.
const released = createGzip();
(released as any)._handle.close();
(released as any)._handle = null;
console.log("released:", released.destroyed, released.closed, (released as any)._closed);
released.destroy();
console.log("released destroy:", released.destroyed, released.closed, (released as any)._closed);


let complete: any;
const events: string[] = [];
const deferred = new Transform({ destroy(error, callback) { complete = callback; } });
deferred.on("close", () => events.push("close"));
console.log("deferred return:", deferred.destroy() === deferred);
console.log("deferred pending:", deferred.destroyed, deferred.closed, events.join(","));
deferred.destroy();
complete(null);
console.log("deferred complete:", deferred.destroyed, deferred.closed, events.join(","));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("deferred events:", events.join(","));

const quiet = new Transform({ emitClose: false });
let quietCloses = 0;
quiet.on("close", () => quietCloses++);
quiet.destroy();
console.log("quiet complete:", quiet.destroyed, quiet.closed);
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("quiet events:", quietCloses);
