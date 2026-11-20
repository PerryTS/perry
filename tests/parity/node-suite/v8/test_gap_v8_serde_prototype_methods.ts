// The v8 serializer/deserializer classes expose their methods through the
// canonical prototypes: the method a receiver sees IS the prototype's, the
// receiver's [[Prototype]] is its constructor's, and the Default* subclasses keep
// that chain whether they are built with `new`, a dynamic alias, or
// `Reflect.construct`. Buffers come from `v8.serialize` because the class-based
// write path is not implemented.
import v8 from "node:v8";
const s: any = new v8.Serializer();
const other: any = new v8.Serializer();
console.log("methods", typeof s.writeHeader, s.writeHeader === other.writeHeader,
  s.writeHeader === v8.Serializer.prototype.writeHeader);
console.log("prototype", Object.getPrototypeOf(s) === v8.Serializer.prototype);
console.log("method-lengths", s.writeHeader.length, s.writeValue.length, s.writeUint64.length);
const buffer = v8.serialize({ label: "persist" });
const d: any = new v8.Deserializer(buffer);
console.log("deserializer-prototype", Object.getPrototypeOf(d) === v8.Deserializer.prototype,
  d.readHeader === v8.Deserializer.prototype.readHeader);
const Alias: any = Reflect.get(v8, "DefaultSerializer");
const a: any = new Alias();
console.log("default-prototypes", Object.getPrototypeOf(a) === v8.DefaultSerializer.prototype,
  Object.getPrototypeOf(v8.DefaultSerializer.prototype) === v8.Serializer.prototype,
  Object.getPrototypeOf(v8.DefaultSerializer) === v8.Serializer,
  a.writeHeader === v8.Serializer.prototype.writeHeader);
const Direct: any = new v8.DefaultDeserializer(buffer);
const Dynamic: any = Reflect.construct(v8.DefaultDeserializer, [buffer]);
console.log("default-deserializer-prototype",
  Object.getPrototypeOf(Direct) === v8.DefaultDeserializer.prototype,
  Object.getPrototypeOf(Dynamic) === v8.DefaultDeserializer.prototype,
  Object.getPrototypeOf(v8.DefaultDeserializer.prototype) === v8.Deserializer.prototype);
try { v8.Serializer.prototype.writeHeader.call({}); }
catch (error: any) { console.log("brand", error.name); }
