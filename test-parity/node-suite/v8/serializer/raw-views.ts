import { Deserializer, Serializer } from "node:v8";

const bytes = Buffer.from([99, 10, 20, 30, 88]);
const views = [
  bytes.subarray(1, 4),
  new Uint8Array(bytes.buffer, bytes.byteOffset + 1, 3),
  new DataView(bytes.buffer, bytes.byteOffset + 1, 3),
];

for (const view of views) {
  const serializer = new Serializer();
  serializer.writeRawBytes(view);
  const deserializer = new Deserializer(serializer.releaseBuffer());
  console.log(
    view.constructor.name + ":",
    [...deserializer.readRawBytes(3)].join(","),
  );
}
