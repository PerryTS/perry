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
const wire = serializer.releaseBuffer();
const deserializer = new Deserializer(wire);
deserializer.readHeader();
deserializer.transferArrayBuffer(7, target);
const result: any = deserializer.readValue();
console.log("target identity:", result.buffer === target);
console.log("target bytes:", [...new Uint8Array(result.buffer)].join(","));

// A transfer id the receiving side never registered fails the read, and ids
// convert with ToUint32 (2 ** 32 + 7 is 7).
const unregistered = new Deserializer(wire);
unregistered.readHeader();
try {
  unregistered.readValue();
  console.log("unregistered: no throw");
} catch (error: any) {
  console.log("unregistered:", error.name, error.message);
}
const wrapped = new Deserializer(wire);
wrapped.readHeader();
wrapped.transferArrayBuffer(2 ** 32 + 7, target);
console.log("wrapped id:", (wrapped.readValue() as any).buffer === target);
