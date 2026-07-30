import * as http2 from "node:http2";

const descriptor = Object.getOwnPropertyDescriptor(http2, "sensitiveHeaders");
console.log("symbol:", typeof http2.sensitiveHeaders);
console.log("global:", Symbol.keyFor(http2.sensitiveHeaders));
console.log("description:", http2.sensitiveHeaders.description);
console.log(
  "descriptor:",
  descriptor?.enumerable,
  descriptor?.writable,
  descriptor?.configurable,
);
