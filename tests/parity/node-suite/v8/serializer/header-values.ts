import { DefaultDeserializer, DefaultSerializer } from "node:v8";

const serializer = new DefaultSerializer();
console.log("writeHeader:", serializer.writeHeader());
for (
  const value of [undefined, null, true, 42, "text", {
    nested: [1, 2],
  }] as const
) {
  console.log("writeValue:", serializer.writeValue(value));
}
const buffer = serializer.releaseBuffer();
console.log("buffer:", Buffer.isBuffer(buffer), buffer.length > 0);

const deserializer = new DefaultDeserializer(buffer);
console.log("readHeader:", deserializer.readHeader());
for (let i = 0; i < 6; i++) {
  const value: any = deserializer.readValue();
  console.log("readValue:", typeof value, JSON.stringify(value));
}
