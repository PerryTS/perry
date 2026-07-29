import { gunzip } from "node:zlib";

const order: string[] = [];
await new Promise<void>((resolve) => {
  gunzip(Buffer.from("invalid"), function (this: any, error, output) {
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
