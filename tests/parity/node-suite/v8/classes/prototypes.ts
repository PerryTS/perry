import * as v8 from "node:v8";

for (
  const name of [
    "Serializer",
    "Deserializer",
    "DefaultSerializer",
    "DefaultDeserializer",
    "GCProfiler",
  ] as const
) {
  const Constructor = v8[name];
  console.log(name, typeof Constructor, Constructor.length, Constructor.name);
  console.log(
    name + " prototype:",
    Object.getOwnPropertyNames(Constructor.prototype).join(","),
  );
  const descriptor = Object.getOwnPropertyDescriptor(Constructor, "prototype")!;
  console.log(
    name + " prototype descriptor:",
    descriptor.writable,
    descriptor.enumerable,
    descriptor.configurable,
  );
}

console.log(
  "serializer inheritance:",
  Object.getPrototypeOf(v8.DefaultSerializer.prototype) ===
    v8.Serializer.prototype,
);
console.log(
  "deserializer inheritance:",
  Object.getPrototypeOf(v8.DefaultDeserializer.prototype) ===
    v8.Deserializer.prototype,
);
