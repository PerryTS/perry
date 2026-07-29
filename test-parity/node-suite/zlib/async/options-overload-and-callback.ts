import { gzip } from "node:zlib";

const order: string[] = [];
const settled = new Promise<void>((resolve) => {
  gzip("callback contract", { level: 1 }, function (this: any, error, output) {
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
