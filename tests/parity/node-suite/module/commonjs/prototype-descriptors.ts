import { Module } from "node:module";

for (const key of Object.getOwnPropertyNames(Module.prototype).sort()) {
  const descriptor = Object.getOwnPropertyDescriptor(Module.prototype, key)!;
  const value = "value" in descriptor ? descriptor.value : undefined;
  console.log(
    key,
    JSON.stringify({
      type: typeof value,
      name: typeof value === "function" ? value.name : "",
      length: typeof value === "function" ? value.length : -1,
      enumerable: descriptor.enumerable,
      writable: descriptor.writable,
      configurable: descriptor.configurable,
      get: typeof descriptor.get,
      set: typeof descriptor.set,
    }),
  );
}
