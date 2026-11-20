import { gunzip, Gunzip } from "node:zlib";

import { Transform } from "node:stream";

let receiver: any;
const order: string[] = [];
await new Promise<void>((resolve) => {
  gunzip(Buffer.from("invalid"), function (this: any, error, output) {
    receiver = this;
    order.push("callback");
    console.log("error:", error?.name, error?.code);
    console.log("output undefined:", output === undefined);
    console.log("arguments length:", arguments.length);
    console.log(
      "receiver:",
      this === undefined ? "undefined" : this?.constructor?.name,
    );
    resolve();
  });
  order.push("after-call");
});
console.log("order:", order.join(","));

// Public identity and inherited events must belong to the completed engine.
console.log("engine instance:", receiver instanceof Gunzip, receiver instanceof Transform);
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

// Malformed prefixes differ from valid gzip headers cut before completion.
const prefixes: Array<[string, number[]]> = [
  ["bad-first", [0]],
  ["bad-magic", [0, 0]],
  ["bad-second", [31, 0]],
  ["bad-method", [31, 139, 0]],
  ["valid-magic-prefix", [31]],
  ["valid-magic", [31, 139]],
  ["valid-method", [31, 139, 8]],
  ["valid-fixed-header", [31, 139, 8, 0, 0, 0, 0, 0, 0, 3]],
];
for (const [label, bytes] of prefixes) {
  await new Promise<void>((resolve) => {
    gunzip(Buffer.from(bytes), function (this: any, error, output) {
      console.log("prefix:", label, error?.name, error?.code,
        output === undefined, arguments.length, this instanceof Gunzip);
      resolve();
    });
  });
}
