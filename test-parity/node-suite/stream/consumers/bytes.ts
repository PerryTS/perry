import { Buffer } from "node:buffer";
import { Readable } from "node:stream";
import { bytes } from "node:stream/consumers";

const value = await bytes(
  Readable.from([Buffer.from("uv"), new Uint8Array([119, 120])]),
);
console.log("bytes length:", value.length);
console.log("bytes first:", value[0]);
console.log("bytes last:", value[3]);
