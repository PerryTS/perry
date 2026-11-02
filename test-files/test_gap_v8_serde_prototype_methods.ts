// The v8 serializer/deserializer classes expose their methods through the
// canonical prototypes, so the method a receiver sees IS the prototype's, and
// the receiver's [[Prototype]] is its constructor's. The dynamic-alias block of
// the original regression (`Reflect.get` / `Reflect.construct`) needs the ABI
// change and lives in its own slice.
import v8 from "node:v8";
const s: any = new v8.Serializer();
const other: any = new v8.Serializer();
console.log("methods", typeof s.writeHeader, s.writeHeader === other.writeHeader,
  s.writeHeader === v8.Serializer.prototype.writeHeader);
console.log("prototype", Object.getPrototypeOf(s) === v8.Serializer.prototype);
console.log("method-lengths", s.writeHeader.length, s.writeValue.length, s.writeUint64.length);
// Built from a Buffer rather than from `s.releaseBuffer()`: the serializer's
// release path is a separate (unsupported) operation, and this slice is about
// the prototypes, not about which operations the backend implements.
const d: any = new v8.Deserializer(Buffer.from([]));
console.log("deserializer-prototype", Object.getPrototypeOf(d) === v8.Deserializer.prototype);
try { v8.Serializer.prototype.writeHeader.call({}); }
catch (error: any) { console.log("brand", error.name); }
