import v8 from "node:v8";
const s: any = new v8.Serializer();
const other: any = new v8.Serializer();
console.log("methods", typeof s.writeHeader, s.writeHeader === other.writeHeader,
  s.writeHeader === v8.Serializer.prototype.writeHeader);
console.log("prototype", Object.getPrototypeOf(s) === v8.Serializer.prototype);
console.log("method-lengths", s.writeHeader.length, s.writeValue.length, s.writeUint64.length);
s.writeHeader();
s.writeUint32(42);
s.writeUint64(1, 2);
s.writeDouble(1.5);
s.writeRawBytes(Buffer.from([3, 4]));
console.log("write", s.writeValue({ label: "persist" }));
const buffer = s.releaseBuffer();
const d: any = new v8.Deserializer(buffer);
console.log("read", d.readHeader(), d.readUint32(), JSON.stringify(d.readUint64()),
  d.readDouble(), d.readRawBytes(2).toString("hex"), d.readValue().label);
console.log("deserializer-prototype", Object.getPrototypeOf(d) === v8.Deserializer.prototype);
const Alias: any = Reflect.get(v8, "DefaultSerializer");
const a: any = new Alias();
console.log("default-prototypes", Object.getPrototypeOf(a) === v8.DefaultSerializer.prototype,
  Object.getPrototypeOf(v8.DefaultSerializer.prototype) === v8.Serializer.prototype,
  Object.getPrototypeOf(v8.DefaultSerializer) === v8.Serializer);
a.writeHeader();
a.writeValue({ value: 27 });
const b = a.releaseBuffer();
const Direct: any = new v8.DefaultDeserializer(b);
const Dynamic: any = Reflect.construct(v8.DefaultDeserializer, [b]);
console.log("default-read", Direct.readHeader(), Direct.readValue().value,
  Dynamic.readHeader(), Dynamic.readValue().value);
console.log("default-deserializer-prototype",
  Object.getPrototypeOf(Direct) === v8.DefaultDeserializer.prototype,
  Object.getPrototypeOf(Dynamic) === v8.DefaultDeserializer.prototype,
  Object.getPrototypeOf(v8.DefaultDeserializer.prototype) === v8.Deserializer.prototype);
try { v8.Serializer.prototype.writeHeader.call({}); }
catch (error: any) { console.log("brand", error.name); }
