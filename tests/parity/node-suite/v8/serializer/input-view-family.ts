import { deserialize, serialize } from "node:v8";

const payload = serialize("xxxx");
console.log(
  "aligned payload:",
  payload.byteLength,
  payload.byteLength % 8 === 0,
);

const constructors = [
  Int8Array,
  Uint8Array,
  Uint8ClampedArray,
  Int16Array,
  Uint16Array,
  Int32Array,
  Uint32Array,
  Float16Array,
  Float32Array,
  Float64Array,
  BigInt64Array,
  BigUint64Array,
  DataView,
];
for (const Constructor of constructors) {
  const bytesPerElement = (Constructor as any).BYTES_PER_ELEMENT ?? 1;
  const view = Constructor === DataView
    ? new DataView(payload.buffer, payload.byteOffset, payload.byteLength)
    : new (Constructor as any)(
      payload.buffer,
      payload.byteOffset,
      payload.byteLength / bytesPerElement,
    );
  console.log(Constructor.name + ":", deserialize(view) === "xxxx");
}
