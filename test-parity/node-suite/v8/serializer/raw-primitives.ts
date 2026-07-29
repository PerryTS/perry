import { Deserializer, Serializer } from "node:v8";

const serializer = new Serializer();
serializer.writeUint32(0);
serializer.writeUint32(0xffffffff);
serializer.writeUint64(0x12345678, 0x90abcdef);
serializer.writeDouble(-0.25);
serializer.writeRawBytes(new Uint8Array([1, 2, 255]));
const buffer = serializer.releaseBuffer();
console.log("buffer:", Buffer.isBuffer(buffer), buffer.length);

const deserializer = new Deserializer(buffer);
console.log("uint32:", deserializer.readUint32(), deserializer.readUint32());
console.log(
  "uint64:",
  deserializer.readUint64().map((value) => value.toString(16)).join(","),
);
console.log("double:", deserializer.readDouble());
console.log("raw:", [...deserializer.readRawBytes(3)].join(","));
