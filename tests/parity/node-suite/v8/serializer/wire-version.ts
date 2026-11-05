import { DefaultDeserializer, DefaultSerializer } from "node:v8";

const serializer = new DefaultSerializer();
serializer.writeHeader();
serializer.writeValue("wire");
const deserializer = new DefaultDeserializer(serializer.releaseBuffer());

console.log("before header:", deserializer.getWireFormatVersion());
console.log("header:", deserializer.readHeader());
const version = deserializer.getWireFormatVersion();
console.log(
  "version:",
  typeof version,
  Number.isInteger(version),
  version === 15,
);
console.log("value:", deserializer.readValue());
