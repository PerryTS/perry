import * as v8 from "node:v8";

const serializer = new v8.DefaultSerializer();
const serialized = v8.serialize({ ok: true });
const deserializer = new v8.DefaultDeserializer(serialized);
const profiler = new v8.GCProfiler();

console.log(
  "serializer:",
  serializer instanceof v8.DefaultSerializer,
  serializer instanceof v8.Serializer,
);
console.log(
  "deserializer:",
  deserializer instanceof v8.DefaultDeserializer,
  deserializer instanceof v8.Deserializer,
);
console.log("profiler:", profiler instanceof v8.GCProfiler);
console.log(
  "constructors:",
  serializer.constructor.name,
  deserializer.constructor.name,
  profiler.constructor.name,
);
console.log("independent:", new v8.Serializer() !== new v8.Serializer());
