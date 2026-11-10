import { Http2ServerResponse } from "node:http2";
import { Stream } from "node:stream";

console.log("name:", Http2ServerResponse.name);
console.log("length:", Http2ServerResponse.length);
console.log("stream:", Http2ServerResponse.prototype instanceof Stream);
console.log(
  "methods:",
  ["setHeader", "writeHead", "write", "end"].map((key) =>
    typeof (Http2ServerResponse.prototype as any)[key]
  ).join(","),
);
