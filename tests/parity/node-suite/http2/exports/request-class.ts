import { Http2ServerRequest } from "node:http2";
import { Readable } from "node:stream";

console.log("name:", Http2ServerRequest.name);
console.log("length:", Http2ServerRequest.length);
console.log("readable:", Http2ServerRequest.prototype instanceof Readable);
console.log(
  "methods:",
  ["setTimeout", "destroy", "_read"].map((key) =>
    typeof (Http2ServerRequest.prototype as any)[key]
  ).join(","),
);
