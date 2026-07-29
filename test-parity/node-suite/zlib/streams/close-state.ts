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
