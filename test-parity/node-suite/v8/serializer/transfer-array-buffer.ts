import { Deserializer, Serializer } from "node:v8";

const source = new Uint8Array([1, 2, 3]).buffer;
const serializer = new Serializer();
serializer.writeHeader();
serializer.transferArrayBuffer(7, source);
serializer.writeValue({ buffer: source });
console.log(
  "source intact:",
  source.byteLength,
  [...new Uint8Array(source)].join(","),
);

const target = new Uint8Array([9, 8, 7]).buffer;
const deserializer = new Deserializer(serializer.releaseBuffer());
deserializer.readHeader();
deserializer.transferArrayBuffer(7, target);
const result: any = deserializer.readValue();
console.log("target identity:", result.buffer === target);
console.log("target bytes:", [...new Uint8Array(result.buffer)].join(","));
