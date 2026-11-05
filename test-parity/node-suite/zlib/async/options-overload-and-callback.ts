import { gzip, Gzip } from "node:zlib";

import { Transform } from "node:stream";

let receiver: any;
const order: string[] = [];
const settled = new Promise<void>((resolve) => {
  gzip("callback contract", { level: 1 }, function (this: any, error, output) {
    receiver = this;
    order.push("callback");
    console.log("error:", error === null ? "null" : error?.name);
    console.log("output buffer:", Buffer.isBuffer(output));
    console.log("arguments length:", arguments.length);
    console.log(
      "receiver:",
      this === undefined ? "undefined" : this?.constructor?.name,
    );
    resolve();
  });
});
order.push("after-call");
await settled;
console.log("order:", order.join(","));

// Public identity and inherited events must belong to the completed engine.
console.log("engine instance:", receiver instanceof Gzip, receiver instanceof Transform);
console.log("engine bytesWritten:", receiver?.bytesWritten);
if (receiver && typeof receiver.on === "function" &&
    typeof receiver.emit === "function" && typeof receiver.removeListener === "function") {
  const events: string[] = [];
  function listener(this: any, value: string) {
    events.push(`${this === receiver}:${value}`);
  }
  const chained = receiver.on("callback-contract", listener) === receiver;
  const first = receiver.emit("callback-contract", "payload");
  const removed = receiver.removeListener("callback-contract", listener) === receiver;
  const second = receiver.emit("callback-contract", "ignored");
  console.log("engine events:", chained, first, removed, second, events.join("|"));
} else {
  console.log("engine events:", "missing");
}
