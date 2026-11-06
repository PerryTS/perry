import * as v8 from "node:v8";

for (
  const key of [
    "serialize",
    "Serializer",
    "promiseHooks",
    "startupSnapshot",
    "GCProfiler",
  ] as const
) {
  const descriptor = Object.getOwnPropertyDescriptor(v8, key)!;
  console.log(
    key,
    descriptor.enumerable,
    descriptor.configurable,
    "writable" in descriptor ? descriptor.writable : "accessor",
    typeof descriptor.value,
  );
}

console.log("tag:", Object.prototype.toString.call(v8));
console.log("extensible:", Object.isExtensible(v8));
