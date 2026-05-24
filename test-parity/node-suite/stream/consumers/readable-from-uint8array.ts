import { Readable } from "node:stream";
import { arrayBuffer, buffer, bytes, text } from "node:stream/consumers";

const source = new Uint8Array([65, 66]);

const ab = await arrayBuffer(Readable.from(source));
console.log("arrayBuffer bytes:", Array.from(new Uint8Array(ab)).join(","));

const buf = await buffer(Readable.from(source));
console.log("buffer bytes:", Array.from(buf).join(","));

const by = await bytes(Readable.from(source));
console.log("bytes bytes:", Array.from(by).join(","));

try {
  await text(Readable.from(source));
  console.log("text resolved");
} catch (e) {
  const err = e as Error & { code?: string };
  console.log("text rejected:", err.name);
  console.log("text code:", err.code ?? "");
}
