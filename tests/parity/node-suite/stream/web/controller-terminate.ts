import { TransformStream } from "node:stream/web";
// TransformStreamDefaultController.terminate() ends the readable side and
// errors the writable side.
const ts = new TransformStream({
  start(controller) { controller.terminate(); },
});
const reader = ts.readable.getReader();
const r = await reader.read();
console.log("done:", r.done);
