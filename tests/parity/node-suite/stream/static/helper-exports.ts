import stream from "node:stream";
import * as streamNs from "node:stream";

const helpers = [
  ["_isArrayBufferView", (stream as any)._isArrayBufferView],
  ["_isUint8Array", (stream as any)._isUint8Array],
  ["_uint8ArrayToBuffer", (stream as any)._uint8ArrayToBuffer],
  ["isDestroyed", (stream as any).isDestroyed],
] as const;

for (const [name, fn] of helpers) {
  console.log(
    "helper:",
    name,
    Object.keys(stream as any).includes(name),
    Object.keys(streamNs as any).includes(name),
    typeof fn,
    fn.length,
  );
}

const u8 = new Uint8Array([1, 2, 3]);
const buffer = Buffer.from([4, 5]);
const dataView = new DataView(new ArrayBuffer(2));

console.log(
  "isUint8Array:",
  (stream as any)._isUint8Array(u8),
  (stream as any)._isUint8Array(buffer),
  (stream as any)._isUint8Array(dataView),
);
console.log(
  "isArrayBufferView:",
  (stream as any)._isArrayBufferView(u8),
  (stream as any)._isArrayBufferView(buffer),
  (stream as any)._isArrayBufferView(dataView),
  (stream as any)._isArrayBufferView(new ArrayBuffer(1)),
);

const fromU8 = (stream as any)._uint8ArrayToBuffer(u8);
const fromDataView = (stream as any)._uint8ArrayToBuffer(dataView);
console.log("toBuffer u8:", Buffer.isBuffer(fromU8), fromU8.toString("hex"));
console.log(
  "toBuffer dataview:",
  Buffer.isBuffer(fromDataView),
  fromDataView.length,
);

const readable = stream.Readable.from(["x"]);
console.log("destroyed before:", (stream as any).isDestroyed(readable));
readable.destroy();
console.log("destroyed after:", (stream as any).isDestroyed(readable));
console.log("destroyed plain:", (stream as any).isDestroyed({ destroyed: true }));
