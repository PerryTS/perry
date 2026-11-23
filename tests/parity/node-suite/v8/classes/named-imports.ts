// Named (and renamed) imports construct the same serde instances as the
// namespace form, including the GCProfiler sibling.
import * as v8 from "node:v8";
import {
  DefaultDeserializer,
  DefaultSerializer,
  Deserializer,
  GCProfiler,
  Serializer as RenamedSerializer,
} from "node:v8";

const buffer = v8.serialize({ label: "named" });
const ds: any = new DefaultSerializer();
const s: any = new RenamedSerializer();
const dd: any = new DefaultDeserializer(buffer);
const d: any = new Deserializer(buffer);
console.log("methods", typeof ds.writeHeader, typeof s.writeValue,
  typeof dd.readHeader, typeof d.readValue);
console.log("prototypes",
  Object.getPrototypeOf(ds) === v8.DefaultSerializer.prototype,
  Object.getPrototypeOf(s) === v8.Serializer.prototype,
  Object.getPrototypeOf(dd) === v8.DefaultDeserializer.prototype,
  Object.getPrototypeOf(d) === v8.Deserializer.prototype);
console.log("shared", ds.writeHeader === v8.Serializer.prototype.writeHeader);
const p: any = new GCProfiler();
console.log("profiler", typeof p.start, typeof p.stop);
