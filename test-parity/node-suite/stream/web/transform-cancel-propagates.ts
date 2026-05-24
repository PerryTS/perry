import { TransformStream } from "node:stream/web";
// readable.cancel() on a TransformStream propagates to writable —
// further writes should be rejected (writer.closed promise rejects).
const ts = new TransformStream();
const reader = ts.readable.getReader();
await reader.cancel("downstream-stop");
const writer = ts.writable.getWriter();
let rejected = false;
try {
  await writer.write("late");
} catch {
  rejected = true;
}
console.log("write rejected after cancel:", rejected);
